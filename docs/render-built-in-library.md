# Built-in authored render library

**Integration-batch source, not yet a validated application release.** The application-facing adapters below are prepared in the personal render branch. Their compilation, regression execution and visual acceptance must be recorded on the frozen batch. Source-resource conformance from PR #6 is separate evidence.

## Inventory and availability

The Render panel now has a library section using the existing dropdown, list-picker, disclosure, number-field and theme-token components. Search is paginated; target search returns bounded name/ordinal pages rather than mesh payloads. The same operations are available through engine commands, CLI and MCP.

| Source | Prepared application projection | Explicit exclusions |
|---|---|---|
| 48 authored PBR surfaces | Uniform opaque assignment, or GPU planar color/normal/roughness/metallic maps | Height/displacement, manufacturing geometry and true anisotropic BRDF |
| 48 SELENE appearance recipes | Original uniform opaque baseline | The advanced-target properties listed in each recipe |
| 24 HDR environment fields | Hash-verified, embedded CPU/GPU environment in a saved setup | Finite room geometry, parallax, contact floor and measured photometry |
| Numeric rectangular/sun rigs | Exact emitter conversion at an explicit subject scale and center | Passive cards require an explicitly acknowledged emitters-only variant; calibration-blocked and externally owned sun rigs refuse |
| 600 material, 280 lighting, 280 environment concepts; 24 scene and 10 quality specifications | Search and inspect the original specification and capability requirements | Not represented as complete executable presets without their required features and target bindings |

Original authoring manifests remain unchanged, including their `runtime_ready: false` source labels. The engine returns a separate projection record describing the adapter and its limitations. No mass, density, elastic property, motion parameter or manufacturing claim is inferred from a visual resource.

## Commands

- `render.library.list`: `kind`, bounded `query`, `offset`, `limit` (1..128). Returns summaries, original status, runtime projection, total and next page.
- `render.library.describe`: exact `kind` and `id`; returns the original entry and explicit variants/omissions.
- `render.library.targets`: bounded body-name query and page; returns current ordinals, names, revision and captured scene bounds.
- `render.library.appearance.apply`: `id`, `variant: "uniform_opaque"`, and 1..256 exact `bodies`. Uses ordinary appearance assignment, one undo, with explicit approximation acknowledgement.
- `render.library.surface.prepare`: `id`, exact `body_index` and `body_name`, optional origin, plane and repeat scale. Returns a GPU texture binding; does not assign a material or alter the document. The default origin is the target bounds center. Existing body IOR is retained.
- `render.library.environment.apply`: `id`, setup `name`, optional typed setup `set` patch, authored rotation/intensity and explicit `replace_environment`. Embeds the original HDR and saves the setup atomically. Requires a CPU/GPU job but no installed device or source file.
- `render.library.studio.prepare`: `id`, explicit `variant`, `center_mm` and `subject_extent_mm`. Returns numerical GPU lighting plus limitations and a geometry-clearance review requirement.
- `render.library.studio.apply`: a prepared rig request, setup name/patch and `replace_lighting_and_environment: true`. Saves the selected emitter projection in one undo. Existing environment and lights are replaced only with that explicit confirmation.

The library browser only enables executable actions for their supported categories and selected executor. A stale target requires a fresh selection; an authored surface cannot silently replace a glass binding. GPU surface bindings remain temporary request state, as with existing optical bindings. Built-in image sources no longer depend on filesystem paths, but their body bindings are not yet document-portable.

## Reproducible embedded sources

`tools/render-library/generate.py` validates every included source against its manifest SHA-256 and generates `crates/engine/src/cmd/render/library_data.rs`. Use `--check` to verify that the lookup is current. No Python interpreter is required at application runtime.

The generated lookup embeds 192 eight-bit surface maps and 24 original HDR files. It deliberately excludes the 48 sixteen-bit height maps from runtime use. Those height files remain in the authored pack and its independent conformance path; they are not passed through an eight-bit color decoder.

Runtime texture identifiers use the fixed syntax `builtin://SURF-001/base_color`, with corresponding `normal_open_gl`, `roughness` and `metallic` channels. The identifier is matched against generated constants, never joined to a user-controlled filesystem path. The requested channel role must match. Capture checks the encoded digest and byte admission before retaining an immutable buffer. External files retain their existing scoped VFS route.

## Color, normal and environment conventions

Authored base color is **absolute reflectance**, not a multiplier for another body's tint. The new explicit `base_color_mode: "replace"` implements that projection; omitted/default `"modulate"` preserves existing external-map behavior. Alpha, dielectric flags, engineering material properties and emission are not replaced by a base-color map.

Roughness and metallic maps are decoded as **linear data**. Their scalar channel replaces the corresponding optical parameter. Roughness retains the renderer's existing 0.045 minimum; this is a documented transport boundary, not a claim of perfect delta reflection. Other channels remain unchanged.

The authoring normal convention is OpenGL +Y with +V toward the image top. `flip_v: true` reverses the image lookup row while preserving the world-space +V normal basis. Legacy external maps keep `flip_v: false`. Planar frames use world millimetres and preserve the authored repeat scale; the UI exposes their origin and U/V vectors.

The authored HDR longitude is `phi = 2*pi*(u - 0.5)`, while the existing renderer uses `phi = 2*pi*u`. The built-in adapter therefore adds an explicit pi-radian native rotation. The apply reply reports the author's rotation, the native rotation and the offset. Original encoded bytes and hashes do not change. The stored setup contains the native angle, so cold reopening does not need a special decoder or the original source path.

## Shared texture storage and finite admission

Up to 128 exact body bindings may share immutable captured resources. Within one capture, a file/role or built-in identifier/role is read once. `RuntimeTextures` canonicalizes identical encoded content **and role** to one immutable source; the GPU packer decodes and uploads that same identity only once. A different interpretation is never deduplicated merely because its encoded bytes match.

Unique encoded sources are bounded at 64 MiB and unique decoded linear pixels at 96 MiB. Geometry, packing/codec working storage, film and retained results still have to fit the unchanged GPU host/job budgets. The maxima are not simultaneously guaranteed. Reusing one finish on many bodies avoids multiplying its texture payload; using many distinct finishes can still reach admission limits. No silent texture downscaling, channel dropping or CPU fallback occurs.

## Lighting conversion and remaining review

The authored rig fixture extent is 1000 mm. Positions and full emitter dimensions scale together to the requested subject extent; GPU area emitters receive half-edge vectors, not full widths. The authored incoming photon direction is negated to obtain the renderer's direction toward the sun, and angular diameter is halved to radius. Radiance remains in authored relative linear-sRGB units, not lux or watts.

A saved HDR with an authored solar disk cannot also retain an analytic sun. Applying a studio explicitly replaces prior lighting and environment to establish one energy owner. Passive bounce/flag cards are geometry, not lights; the emitters-only action explicitly acknowledges their omission. It is not advertised as a complete reproduction of a card-dependent studio. Emitter/object clearance requires review at the chosen placement; the adapter does not claim collision-free lighting geometry.

## Prepared verification

Regression sources cover catalog counts/pagination, blocked concepts, exact hashes and channel roles, absent-file HDR persistence, orientation conversion, explicit appearance approximation, unchanged engineering metadata, body identity, scalar channels, positive normal basis, actual shared source storage, GPU buffer deduplication and GPU scalar output against a linear reference. Execution results are recorded separately after the batch is frozen.
