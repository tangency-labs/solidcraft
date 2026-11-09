# Handoff to the render/materials implementation thread

## Scope of this contribution

This PR provides English planning content under `docs/render-materials/` and native SELENE authoring resources plus development tools under `assets/render/selene/`, with exact attribution rows. It changes no application command, shader, renderer backend or native document format. The SELENE continuation reconstructs geometry, assemblies and appearances in a derivative through existing SolidCraft commands; the broader catalog remains a specification backlog. The integration base is the user's `lab/render-materials` branch at the recorded revision. Do not replace it with a newer upstream snapshot as a side effect of integrating this package.

## Immediate implementation tasks

| Order | Task | Acceptance |
|---|---|---|
| 1 | Inventory the actual landed backend and settings support. | Mark each CAP record present, partial or missing with evidence; concept rows remain unavailable. |
| 2 | Define versioned appearance, rig, environment and shot resource contracts. | Units, roles, inheritance, locks, provenance and capability requirements round-trip. |
| 3 | Implement a read-only preset resolver and atomic apply through the engine. | The same resolved request reaches UI, CLI and MCP; unsupported input leaves the scene unchanged. |
| 4 | Bind a small representative material/lighting/environment set. | Authored resources, thumbnails and tests demonstrate the claimed capabilities. |
| 5 | Connect library, contextual inspector and final-job preflight. | A first-time user can apply a scene, change a finish, undo and reproduce output. |
| 6 | Establish raw/finished image and numeric reference scenes. | Saved manifests identify source, executor, resources and color treatment. |
| 7 | Bind quality profiles with explicit budgets and resource admission. | Converged, unconverged, canceled and unsupported outcomes are distinguishable. |
| 8 | Integrate SELENE role bindings and procedural lunar stages. | Native hash, model scale, source ownership and material assignments pass review. |
| 9 | Publish the first validated content pack. | Availability is driven by actual resource and executor support. |
| 10 | Develop animation and dynamics in its own implementation arc. | Analytic fixtures and physical contacts pass before producing the throw/catch demo. |

## Architecture boundaries

### Required UI alignment

Use [UI_INTEGRATION.md](UI_INTEGRATION.md) and [ui-integration.json](ui-integration.json) before wiring the library or inspector. The audit compared the fixed 0.18.2 integration base with upstream 0.18.4 at `728f753b8094e1a54c5cc8b7e453982afa028023`, read-only. Preserve the implemented shell: one Render workspace and `RENDER` ribbon tab, current `SCENE`/`IMAGE` groups, shared Floating/Docked layouts, approved icons, theme tokens and UI kit. The selected Render tab uses `render_ink` on `render_soft`; its icons keep their semantic palettes.

Extend the existing Appearance/LocalLibrary consumer and current render commands. Library categories are local navigation; Quick/Detailed/Expert are inspector disclosures. A future result canvas, Render picking/gizmos and migration from the fixed 285-point settings container to the shared Panel are explicit integration tasks. Do not describe them as current support. Reconcile the pinned mappings with the implementation that actually lands in this fork, without replacing its fixed base.

The twelve prepared SELENE shots now have exact three-profile quality recommendations in [the demo flow](../../assets/render/selene/ui/demo-flow.json). Its current state remains unbound, and opening the standalone swatch input must create a fresh isolated document. Runtime validation includes theme/layout/scaling, keyboard, scope, draft, lock and multi-document behavior as specified in the UI acceptance matrix.

### Layer and state ownership

Follow the existing layer direction: rendering primitives and capability contracts remain below the document/engine layers; persisted scene intent belongs at the appropriate document boundary; commands orchestrate changes; UI panels display and invoke commands. Keep device/windowing/UI dependencies out of lower layers according to `AGENTS.md`. Use the existing navigation handler for all new render views.

JSON in this package is a specification format, not a requirement to load unvalidated dictionaries inside the Rust application. Design typed Rust data with finite-value checks, caps, version migration, resource hashes and errors. Bound array lengths, resource size, recursion, texture allocation and loop work before execution. Unknown fields, unavailable features and unresolved resources need clear outcomes; do not silently discard them.

Do not use Python as an application dependency. The supplied validator is a development tool. Do not introduce a second settings implementation in the UI or bypass the command registry to apply catalog entries.

## Stable identity and migrations

Treat concept IDs as durable. Resource versions can evolve without renumbering the concept. An authored material is a versioned realization of a concept, potentially with several executor bindings. A new optical interpretation that changes expected behavior requires a new resource revision and revalidation, not an invisible edit to all saved shots.

Preserve the old ID ranges when moving from the previous planning inventory. Use English display names and searchable aliases where helpful. The retained `LUZ` prefix is an identifier compatibility decision, not a request for Spanish UI text.

## Prepared files available now

Use [assets/render/selene](../../assets/render/selene/README.md) as the prepared resource authority. It contains 48 original recipes with current opaque PBR projections and separate advanced intent, one executable 48-swatch command script, 33 semantic roles, 10 stages and 12 shot recipes. These are authoring resources; do not advertise them as a validated production library or import the pack as scalar `solidcraft-library/1` data.

`tools/prepare.py` inspects native identity and assembly metadata, makes an unreviewed body-role template, and compiles an opaque overlay only after source and target review. It emits the documented `appearance.assign` envelope, preserving physical materials. `tools/validate_prepared.py` checks the prepared package. Both are Python development tools, not application dependencies.

The active [source lock](../../assets/render/selene/source-lock.json) is `pinned_native` and selects the unchanged, owner-supplied [SELENE-showcase.solidcraft](../../assets/render/selene/source/SELENE-showcase.solidcraft). Its exact identity, provenance, notices and executor compatibility are recorded in [SOURCE.md](../../assets/render/selene/SOURCE.md). Source-container mechanism bindings now exist; the derivative's [mechanism work](../../assets/render/selene/MECHANISMS.md), [electronics reconstruction](../../assets/render/selene/ELECTRONICS.md) and [native build tool](../../assets/render/selene/tools/build.py) use the admitted source. Continue verifying exact evaluated targets, joint movement and final-model evidence on the chosen executor. Do not substitute the historical `selene-bindings.json` IDs or infer full-model acceptance from source admission and isolated fixtures.

## Suggested first resource slice

Select exact concept IDs after the material registry is bound, covering: neutral dielectric, coated plastic, clear glass, colored absorbing glass, brushed aluminum, stainless steel, painted steel, sparse-flake paint, rubber, silicone, FDM polymer, PCB solder mask, copper trace, wood with directional grain, a wood coat, LED diffuser, light-guide polymer, solar-cell assembly appearance, thermal blanket and mare/highland regolith.

Pair these with neutral product, dark housing, metal strip, glass bright-field, PCB, wood, LED and spacecraft rigs; use simple physical stages before complex rooms. Publish only the subset supported end to end. Specialized material names must not mask unimplemented optical behavior.

## SELENE-specific dependencies

The still demo needs precise geometry/tessellation, layered surfaces, stable physical mapping, transparent container optics, sampled emitters for LED views, terrain detail, one-Sun ownership, linear color/output and memory-safe quality settings. The future dynamics layer should reuse existing kinematic studies, IK, links, traces and export. It additionally needs verified independent vial bodies, explicit closure and grasp state, collision proxies, mass/inertia, contact, joint actuation, granular approximation choices, analytic verification, immutable bakes and shutter-time sampling.

Do not make animation a prerequisite for the still. Do not advertise the animation as physically verified because it follows a parabola visually: release velocity, rotation, contacts, constraints, mass changes and storage also need evidence.

## Publication and validation

Use [roadmap.json](roadmap.json) for the broader F00–F12 and S01–S06 sequence. The current PR's validation report is [VALIDATION.md](VALIDATION.md). Package validation checks the specification and its references; renderer/physics validation remains work for the corresponding implementation.

Before merge, run the repository's required checks in an environment with the required Rust toolchain and dependencies. `ATTRIBUTION.md` is embedded by the UI and can expand the CI scope even though this contribution contains no application behavior change. Do not report a successful `cargo xtask ci` run unless it actually completed on the submitted tree.
