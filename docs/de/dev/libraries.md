# Extensible libraries: materials, appearances and components

The first [local scalar-library consumer](local-libraries.md) implements only copied
colour/opacity and density/look definitions. Its separate v1 catalog is not the complete
resource-pack/provenance/update system proposed below. Texture/environment descriptors
remain coordinated with the maintainer's renderer work.

Proposal for [#785](https://github.com/bherbruck/solidcraft/issues/785),
[#786](https://github.com/bherbruck/solidcraft/issues/786) and
[#787](https://github.com/bherbruck/solidcraft/issues/787). This page describes planned
contracts and acceptance gates, not an implemented pack format or new commands. It does
not bring the deferred Simulation or Render workspaces into scope.

## Reuse the existing paths

Physical materials already have built-in densities and design-local definitions
(`material.create`, `material.edit`, `material.delete`, `material.list`, `material.assign`).
They drive `component.properties`, survive reopening and participate in configurations.
Appearances have a built-in color/opacity library and body, face and component overrides.
`parts.library`, `parts.insert` and `parts.resize` already generate seven families of
fasteners, pins and bearing envelopes as editable components.

Extend those routes. A shared catalog supplies identity, discovery and provenance;
each entry kind retains its own payload and application rules. A physical material is
not a texture, and a reusable component is not executable plugin code.

## Portable, independently extensible packs

Use one versioned, offline pack envelope for three kinds: physical material, appearance
and component family. Both project contributors and users can author packs without
rebuilding SolidCraft. Start with a directory and JSON manifest, not a new archive format
or an online marketplace. The exact schema belongs in the bounded-loader PR, accompanied
by machine-checked examples; the following are its design requirements:

| Record | Required meaning |
|---|---|
| Pack | Schema version, stable namespaced ID, immutable revision, display name, author, source and license metadata |
| Entry | ID scoped to its pack, immutable revision, kind, label, category/tags, typed payload and per-entry provenance |
| Resource | Relative path, content digest, byte count, media role, author, source, license and included license text where required |
| Dependency | Explicit entry/revision reference; bounded acyclic resolution within explicitly selected packs |
| Selection | Pack ID + entry ID + exact revision; labels and filesystem paths are not identities |

Pack revisions and schema versions are different: the former identifies content, the
latter describes how to read it. Changing content under an existing revision is an error.
Unknown schema versions, entry kinds and required capabilities produce a clear refusal;
they are never silently interpreted as an older format. Optional extensions need an
explicit namespace and preservation rules before being accepted.

Provide three independent scopes:

- **Bundled:** reviewed with SolidCraft and read-only in the library editor.
- **Project:** packs explicitly opened for the current project/team; never discovered
  by silently executing or trusting files beside a design.
- **User:** local packs the user creates, imports, edits or exports. Native file access
  belongs in the app; browser users import/export through the web storage/file workflow.

Discovery order is deterministic. Duplicate labels can coexist, with their source shown.
Duplicate qualified identities/revisions with different contents are errors, not overrides
chosen by scan order. Editing a bundled entry uses **Save a copy** into a user namespace.
Updating a pack is explicit and leaves existing design instances unchanged.

## Designs own their selected definitions

On assignment/insertion, copy the definition, chosen revision, parameters, provenance and
required resource bytes into the design's existing container/resource mechanism. Keep the
source identity for inspection and an eventual explicit Update action. A saved design must
open, rebuild and render the supported appearance without the original pack, network or
another user's home directory. Installing a pack never changes a design or undo history.

Assignment/insertion/update is a normal engine command, atomic and undoable. Preview is
non-mutating. A failed import, unresolved dependency, name collision or unsupported recipe
leaves the document and undo/redo stacks intact. Existing material names and appearance
precedence retain their legacy meaning; introduce stable IDs through a documented migration
rather than reinterpret old name-based assignments. Design-format changes need their own
version/migration and reopen tests, separate from the pack schema version.

## Physical properties (#785)

Keep physical material and appearance selection independent, with an optional default look.
Convert imported density units explicitly to the existing g/cm³ representation; geometry
continues to use millimetres and radians. Never infer units from magnitudes. Properties
need source, edition/date, grade, processing condition and applicable temperature/moisture
when relevant. Unknown properties stay unknown, not zero or guessed values.

Start with density and mass integration through the existing commands. Reserve typed
extensions for elastic constants, yield/ultimate strength, thermal properties and
orthotropic axes, but do not expose these as FEA-ready until their model, units and
validation are implemented. A generic wood density is not an engineering material grade.
User-measured data can be stored with its own provenance and edited without changing the
bundled library.

## Wood and sheet appearances (#786)

Begin with body-local mapping, an explicit physical repeat size and a user-editable grain
frame. Suggest the longest **local** body-bound axis once, with X/Y/Z order for equal
extents; store the chosen frame rather than recompute it on every resize. Manual rotation
and scale survive parameter rebuilds, instance placement, undo and reopening. Shared
component instances transform the same definition-local mapping with their placement.
Non-finite or degenerate frames refuse; automatic choice remains a suggestion.

Visual grain does not rotate the body, its geometric bounding box or a manufacturing
stock frame. Structural grain direction and grain-aware nesting/cut lists are distinct
features, coordinated with [#137](https://github.com/bherbruck/solidcraft/issues/137).
An anisotropic shading effect also does not imply mechanical anisotropy.

After basic wood mapping, add sheet-specific top/bottom/edge looks: MDF, chipboard,
plywood and compact laminate, then melamine, laminate, linoleum and veneer finishes.
Appearance layers initially describe presentation only. Physical finish thickness or
mass must be explicit modelling/material inputs, never an invisible side effect of paint.
Face/edge assignments require durable identities and must refuse a lost association
rather than paint a nearby replacement face.

Keep texture role, color space, normal-map convention and physical scale explicit.
Coordinate with the [render capability contract](https://github.com/bherbruck/solidcraft/blob/main/docs/render-backends.md):
the existing color/opacity viewport is not proof of PBR or texture support. A renderer
must report what it supports; unavailable maps use a documented preview fallback with
a visible notice. Export either preserves supported mapping/resources or reports loss.

## Standard and user component families (#787)

Keep the existing fastener insertion/resize path. Add original parameterized BAR, SHS,
RHS and CHS recipes first, with explicit length units, parameter bounds, section frames
and insertion/connection frames. Validate wall thickness against the outer size and
state whether corners are sharp or rounded. Do not label approximate geometry as a
certified standard section. UB/UC need actual flange/web radii and tapers; identifying a
standard does not grant redistribution rights to its published size tables.

A family revision defines a bounded parameter schema, permitted recipe and optional
reviewed size table. A size designation identifies a row, not an arbitrary filename.
Changing size rebuilds the same component identity and preserves supported connections.
Initial recipes must use a small allowlist of ordinary modelling operations. No embedded
native code, shell commands, scripts, arbitrary command IDs or network hooks. Reject
cycles, excessive expansion and unsupported operations before mutating the design.

Custom assemblies follow single components: bounded dependencies, named connection
frames, explicit shared/independent instance semantics and one-step insertion undo.
Store selected parameters and definition snapshots so missing or changed catalogs cannot
silently substitute another part. Geometric envelope models, including existing bearing
envelopes, must remain distinguishable from detailed/manufacturing-ready models.

## Sources and licensing

SolidCraft is MIT OR Apache-2.0. Code and generated recipes must follow the clean-room
rules; never copy commercial CAD materials, templates, geometry, icons or catalogs.
Third-party assets retain their own licenses. Every bundled resource needs its author,
source, license and `ATTRIBUTION.md` row, even if attribution is optional under CC0.
Record hashes and retrieval dates when admitting actual files. A manifest's claimed
license is provenance to review, not automatic evidence of permission.

Sources checked on 2026-10-13; no external assets or datasets are bundled by this proposal:

| Source | Suitable use / admission condition |
|---|---|
| [ambientCG license](https://docs.ambientcg.com/license/) | Downloadable assets and material preview renders are CC0. Candidate wood/sheet texture source; choose and attribute individual files. |
| [Poly Haven license](https://polyhaven.com/license) | Assets are CC0. Site text, logos and other site content have separate rights; API access has separate terms. Do not copy provider implementation code. |
| [Wikidata structured-data license](https://www.wikidata.org/wiki/Wikidata:Copyright) | Structured entity/property data are CC0; other page text has different terms. Candidate for individual referenced quantities and identifiers, not unverified engineering-grade defaults. Preserve the underlying measurement references, qualifiers and units. |
| [USDA Wood Handbook, revised 2021](https://research.fs.usda.gov/fpl/wood-handbook) | Reference for moisture, physical and mechanical wood properties. Verify the specific table's rights, species, units and conditions before importing data; no wholesale dataset approval is implied. |
| [NIST data/publication rights](https://www.nist.gov/open/copyright-fair-use-and-licensing-statements-srd-data-software-and-technical-series-publications) and [repository terms](https://materialsdata.nist.gov/terms/) | Candidate property references only. Uploaded data use provider-selected terms; public availability is not a blanket redistribution license. Review each dataset. |
| [Khronos PBR overview](https://www.khronos.org/gltf/pbr) | Public rendering conventions for texture roles and shading properties; a design reference, not a source of CAD material data. |
| Original SolidCraft recipes / user measurements | Preferred initial component recipes and illustrative procedural previews; preserve authorship and label illustrative versus measured values. |

Prefer CC0 resources for the starter texture pack and original recipes for the starter
component pack. Defer uncertain or restrictive data rather than scrape supplier sites,
copy standards publications or assume a downloadable CAD file can be redistributed.

## Phases and acceptance gates

Each implementation PR references these issues without closing them prematurely.

| Phase | Deliverable | Required evidence before calling it complete |
|---|---|---|
| 0 | This proposal, source review and roadmap | Maintainer-reviewable contract; distinguish existing functions from planned ones; no runtime or format changes. |
| 1 | Shared bounded manifest parser, resolver and pack validation/import/export | Project and user manifests use the same schema; deterministic conflicts, invalid versions/IDs, missing resources/dependencies, cycles, path traversal and budget exhaustion fail atomically; native + wasm pass. |
| 2 | Material and appearance catalog integration and browser UI | Create/copy/edit/import/export from the UI; existing commands used; analytic box mass before/after assignment; configurations, undo and offline cold reopening; actual screenshots. |
| 3 | Wood mapping and a small reviewed starter pack | Rotate/scale grain on a public-safe block and placed instance; no flip on resize; reopen without pack; rendering capability/fallback tests and screenshots; attribution for each shipped asset. |
| 4 | Parameterized section families and reviewed standard size data | BAR: area = width × height; sharp RHS/SHS: outer area minus inner area; CHS: π(outer_diameter² − inner_diameter²)/4. Check volume = area × length, hollow membership, insertion frame, resize, connections, undo and offline reopening; screenshots. |
| 5 | Custom assemblies, sheet face/edge layers and expanded catalogs | Bounded nested insertion, shared/independent semantics, durable face associations, explicit updates and loss reporting; measured tests and screenshots for each workflow. |

After phase 1, material/appearance integration and section-family work can advance
independently. Wood mapping depends on actual renderer support, not on section families.
Phase 5 is several separate PRs, each following its relevant earlier workflow, rather
than one change coupling assembly insertion to face painting.

Keep parsing/resource I/O in `io`, durable selected definitions in `doc`, application
and validation commands in `engine`, renderer-specific interpretation in `render`/its
adapters, and discovery/editing UI in `ui-egui`. Lower layers do not depend on the UI
or platform file dialogs. Reuse container resource deduplication; avoid a second blob store.

Before allocating/decoding, enforce manifest byte/entry/string/dependency limits, total
resource bytes, image dimensions/decoded bytes and recipe expansion bounds. Paths must be
relative, normalized and confined to the selected pack, including symlink handling. Phase 1
must publish exact limits and test their boundaries. Do not auto-fetch missing files or
install executable extensions. Explicit import/export is enough for the first delivery.

Run `cargo xtask ci` for each implementation arc and check remote PR results. Geometry
tests use analytic values; UI and observable CAD changes include actual app evidence.
The three issues remain open until their individual workflows pass these gates.
