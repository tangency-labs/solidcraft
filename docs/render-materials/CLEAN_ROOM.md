# Clean-room authorship and asset admission

## This contribution

All catalog descriptions, selection criteria, preset numbers, UX flows, analytic fixtures, validation tooling and implementation plans were authored for this SolidCraft contribution from the user's requirements, general rendering and engineering principles, public specifications, scientific facts and the project's own public contracts. Their provenance identifier is `ORIGINAL-2026-10-CATALOG`.

No material library, texture, HDR panorama, template, shader implementation, help passage, screenshot, icon, font or UI artwork was extracted from a commercial CAD or rendering product. No commercial CAD application was driven or automated. No GPL, LGPL or AGPL implementation was read, copied, linked or used as an implementation reference. Product comparison names describe the intended quality bar; they do not authorize borrowing their implementations or content.

The public OpenPBR surface specification was consulted for the existence and scope of a layered material model. This package contains original requirements and prose, not a copy of its specification, artwork, source code or example shaders. A future implementation must independently review the specification and any proposed dependency against the repository policy.

NASA pages were consulted for limited scientific facts. No NASA imagery, terrain file, animation, logo or texture is bundled. The procedural lunar concepts are original proposed scenes, not reconstructed NASA datasets or certified locations. A future terrain or ephemeris import requires its own source, version, rights and coordinate record.

The user's original v7 SELENE model repository remains a pinned historical reference; that legacy file and its demonstration media are not bundled. The current package retains the owner-supplied improved `SELENE-showcase.solidcraft` unchanged under `assets/render/selene/source/`. [SOURCE.md](../../assets/render/selene/SOURCE.md) records its byte identity, author lineage and reviewed notices. Original rover and emblem attribution and the separate Ubuntu Font Licence 1.0 notice for modeled Ubuntu Light lettering accompany the source and must remain with its derivatives. The author's showcase branch does not separately declare a file-specific license for its additions; the source lock preserves that distinction.

The continuation adds original numerical appearance recipes, an analytic-box swatch script, scene/role/shot specifications and native geometry and mechanism authoring tools under `assets/render/selene/`. Tangency's own public mechanism documentation and PR discussions explain the improved variant's documented behavior. Published demonstration media remain reference material and are not copied into this package. The supplied native source has its own pinned identity and retained notices; each separately named derivative must record its own hash and runtime evidence.

The UI continuation uses SolidCraft's own implemented shell, UI kit, theme and accepted SVGs as its design authority. It records a read-only comparison between the fixed integration base and upstream `728f753b8094e1a54c5cc8b7e453982afa028023` in `ui-integration.json`. Original integration prose and metadata are added; no icon master, palette, acceptance hash, commercial reference or application UI implementation is copied or changed. Temporary previews of existing project icons stay outside the shipped resource package.

## What was inspected

- SolidCraft's own `AGENTS.md`, `CONTRIBUTING.md`, architecture, render settings, render workspace, render backend and physical-material-card documentation at the pinned integration base.
- SolidCraft's own attribution, asset validation and CI selection rules to place this contribution correctly.
- SELENE's README, attribution, model statistics and `models/Rover-v7.solidcraft` at the pinned source revision.
- Tangency Labs' own mechanism reference and public showcase, motion export and motion analysis discussions, identified in the prepared source lock.
- The owner-supplied `SELENE-showcase.solidcraft`, its matching author-published Git blob, retained font notice, and official authoring-runtime identity, as recorded in the native package's source lock.
- Public scientific and specification pages identified in [SOURCES.md](SOURCES.md).

The local-only `plan/` directory was absent from the isolated public checkout. This contribution neither invents its contents nor commits a replacement. The user's explicit destination is the personal render/materials fork, so this work targets that branch through a dedicated PR instead of the upstream product repository.

## Resource lifecycle

`concept` → `authored` → `bound` → `validated` → `published`.

A concept contains an intended appearance or environment and its requirements. An authored resource adds actual parameter values, maps or geometry. A bound resource resolves those values against a named executor and resource schema. A validated resource passes recorded visual, numeric, performance and provenance checks. A published resource is admitted to a versioned user-facing pack. No transition follows automatically from generation or import.

## Future procedural and AI-assisted assets

For each authored resource retain:

1. Stable ID, resource version, authoring method, author/reviewer and creation date.
2. Source URLs and revisions, acquired files and hashes, permissions and retained notices where external input exists.
3. Generator version, deterministic seed, parameters and build instructions for procedural work.
4. For AI-assisted work: generator/model version when available, prompt, supplied input rights, output hash, human edits and review outcome. Do not state that the model's training data has been independently audited.
5. Intended map roles, color space, physical scale, orientation, normal convention, tiling policy and resolution.
6. Executor requirements, fallback status, preview evidence and regression scene IDs.

AI-generated material images may contain baked highlights or shadows, inconsistent normal/height information, misleading scale or invented markings. Treat an attractive image as a candidate source, then reconstruct and inspect the surface maps. Never use a single generated beauty image as proof of an energy-consistent material. An AI-generated panorama is not automatically a calibrated HDR light probe; declare its radiance assumptions and verify highlight energy before allowing it to illuminate a physical scene.

Use descriptive physical or manufacturing language for materials. Brand names, certifications, standardized color matches, measured optical constants and space qualification require the corresponding evidence. Appearance presets may suggest a manufactured finish without asserting alloy composition, fire rating, vacuum compatibility, structural strength or flight readiness.

## Attribution and licensing

Original contribution files follow this repository's `MIT OR Apache-2.0` license choice. Exact non-Markdown resource and tooling paths are recorded in the root `ATTRIBUTION.md`; the source ledger records the limited purpose of external references. A reference link does not relicense the referenced work. External content added in a future PR must carry its own explicit license assessment and notice before admission.

## Review checklist

- Review the diff for copied passages, brand assets, untracked binary inputs and unintended source URLs.
- Verify that every shipped external file has provenance and an applicable license; original descriptions are not evidence for third-party map rights.
- Keep concept availability separate from executor support and visual validation.
- Confirm that CAD geometry, engineering properties and the original SELENE source remain unchanged unless a separate, explicit derivative is part of the reviewed task.
- Record actual checks and unresolved limitations in the PR. Do not turn a planning document into a claim of runtime implementation.
