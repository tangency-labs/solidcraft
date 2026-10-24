# Advanced optical producers and catalog closure — 2026-10-23

Owner-directed continuation on the personal `lab/render-scale-resources` branch. This file separates implementation from numerical acceptance and keeps the authored advanced concepts distinct from executable producers. No upstream, main or release changes are authorized by this checkpoint.

## Baseline and preceding native acceptance

Starting source is `adc50cc798e5ed8a4abf1ac22281360b57e6b194`. The isolated native application built from that source successfully completed the adaptive UI render and exported exact PNG/EXR bytes through the actual native file dialogs. A separate job created from the Regrade button retained the same raw EXR. The volume controls were edited in the live Environment inspector; a volume/no-volume comparison changed the raw master, and saving/reopening recovered the exact numerical intent.

These observations close the previously unconfirmed small-scene native workflow. They do not validate the new optical code below. Screenshot PNGs and private execution receipts stay outside production resource folders until reviewed. The full SELENE rerun is a separate acceptance step.

## Advanced-catalog audit

The authored material catalog contains 600 conceptual entries and the capability catalog contains 120 proposed capabilities. They include displacement, subsurface transport, nested dielectrics, dispersion, spectral and interference models, heterogeneous volumes, photometric resources, production workflows and specialized lunar/dynamics requirements. A prose specification is not numerical runtime data. A uniform RGB approximation is not an implementation of all of its requested phenomena.

The audit must produce an explicit per-resource dependency/producer/acceptance mapping. Implementations below are bounded projections, and their limitations must remain discoverable. No blanket `runtime_ready` promotion is made by this batch.

## Certified nested media

A new explicit saved GPU `nested_media` option allows a bounded containment hierarchy of closed dielectric bodies. AABB containment alone is insufficient: every candidate parent is checked for convexity and every child vertex against the parent's outward half-spaces with a strict precision gap. Intersecting, touching, thin-sheet and uncertified boundaries refuse. The validation work has an explicit ceiling and checks cancellation.

The native device receives an immutable reference to each immediate parent medium. The current medium IOR changes directly to the next medium at an interface rather than inserting an artificial air layer. Initialization follows the certified parent chain, including an inside camera. Up to 16 containing layers match the actual device stack.

The existing straight shadow-transmission approximation is not extended by guessing nested Fresnel factors: nested targets using that shortcut refuse. Refracted paths remain authoritative. This is not unrestricted intersecting-media priority handling, nonconvex-parent certification or automatic fusion of touching container/liquid CAD faces.

## Subsurface random walks

A closed dielectric can carry an optional `subsurface` record with a scalar scattering collision rate per millimetre and HG mean cosine. RGB absorption remains a separate coefficient. Free-flight events are sampled inside the actual closed geometry; each event attenuates absorption over the real segment and continues through phase scattering. At the next interface, the existing Fresnel/refraction laws remain active.

This is an original homogeneous random-walk implementation, not a painted translucency filter or a diffusion-profile replacement. Zero scattering retains the prior transport. It does not add heterogeneous density, fluorescence, spectral scattering or a measured material dataset. High optical thickness may require more paths/depth; no convergence guarantee is implied. Surface denoising, multiple conditional transmission strata, exterior-volume combinations and straight transmitted shadows are explicitly refused for active interior scattering until compatible estimators/guides exist.

## Portable exact optical assignments

`RenderSetup.optics` stores a bounded list of authored optical records against the existing definition/feature/component/occurrence identity. Captured mesh ordinals are not persisted. `render.optics.bind` and `render.optics.unbind` are atomic public commands with revision checks and explicit replacement consent. The captured scene resolves exact identities on every Check/Start; missing, replaced or hidden targets refuse instead of transferring to another body.

The optical assignment is separate from CAD geometry, mass and engineering material cards. The shared Nateaux material inspector supplies the numerical controls and explicit Save-to-setup action. Remaining transient assignments are not silently saved or discarded. Exact optical and opaque mapped-surface bindings cannot occupy the same target. The document format changes have pending migration notes rather than an uncoordinated format-version bump.

## Current checkpoint

At this checkpoint the producer code, persistence and shared-UI adapters are implemented, but new focused optical GPU tests and consolidated frozen-source checks have not yet completed. Earlier successful baseline/native runs must not be attributed to this work. Draft commits preserve the work without claiming merge or release approval.

Required acceptance includes: opt-in/default separation, independent body order, strict boundary/overlap refusal, camera-inside transport, analytic nested Beer lengths, matched-IOR furnace energy, zero-rate identity, subsurface energy/absorption bounds, metadata and memory accounting, cancellation, exact save/reopen/undo and actual native controls. New screenshot and resource claims must identify the executable source that produced them.

Public conceptual references (no implementation code copied):
- https://pbr-book.org/3ed-2018/Volume_Scattering
- https://pbr-book.org/3ed-2018/Light_Transport_II_Volume_Rendering/Sampling_Subsurface_Reflection_Functions
- https://pbr-book.org/4ed/Radiometry%2C_Spectra%2C_and_Color/Color
