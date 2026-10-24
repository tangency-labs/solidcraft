# SolidCraft rendering and appearance catalog — v0.2

**An English, clean-room product specification for the personal render/materials fork.**

This contribution defines what to build and how to verify it: 600 appearance concepts, 280 lighting recipes, 280 environments, 120 renderer capabilities, 56 UX requirements and 36 requirements for a future animation and dynamics module. It also supplies numeric preset specifications, scene combinations, current-contract setup examples, and native SELENE reconstruction and validation tools.

The preset set contains **82 future specifications**: 10 quality profiles, 24 studio rigs, 24 environment recipes and 24 complete scene combinations. Six additional current-contract setup examples and six analytic physics fixtures support integration and review.

The catalog is a content and implementation backlog. Its entries are not installed shaders, HDR maps, measured materials, completed terrain assets or evidence of renderer support. The numeric recipes have the status `specified_unbound`: an executor adapter, resources and validation are still required. No production renderer or animation code is changed by this package.

## Start here

| Document | Purpose |
|---|---|
| [Catalog index](CATALOG.md) | Browse every material, lighting recipe and environment by stable ID and family. |
| [Materials guide](materials-guide.md) | Domain coverage, appearance authoring and SELENE material roles. |
| [Lighting guide](lighting-guide.md) | Light recipes, contribution ownership and studio presets. |
| [Environments guide](environments-guide.md) | Procedural stages, lunar environments and environment presets. |
| [Preset contract](PRESETS.md) | Quality tiers, composition rules, units and runtime availability. |
| [UX plan](UX.md) | A few-click workflow with deeper controls available in context. |
| [UI integration](UI_INTEGRATION.md) | Exact alignment with the implemented SolidCraft redesign: ribbon groups, existing icons/tokens, library, inspector, responsiveness and accessibility. |
| [Renderer plan](RENDERER.md) | Quality foundations, expensive features and evidence for competitive claims. |
| [SELENE](SELENE.md) | Improved showcase source selection, lunar stills, model upgrades and future sample-handling dynamics. |
| [Native SELENE package](../../assets/render/selene/README.md) | Pinned source, functional derivative, native verification and render preparation, plus the broader appearance/scene specifications. |
| [Implementation handoff](IMPLEMENTATION_HANDOFF.md) | Integration tasks, boundaries, priorities and acceptance gates. |
| [Clean-room record](CLEAN_ROOM.md) | Authorship, sources, exclusions and future asset-admission rules. |
| [Sources](SOURCES.md) | Public specifications, scientific facts and pinned project references. |

## Machine-readable source of truth

`materials.json`, `lighting.json` and `environments.json` hold the concept inventory. `capabilities.json`, `ux.json`, `simulation-requirements.json` and `roadmap.json` hold requirements and sequencing. `quality-presets.json`, `studio-presets.json`, `environment-presets.json` and `scene-presets.json` hold future preset intent. `baseline-setup-commands.json` contains six examples using the already documented `render.settings.set` shape. `selene-bindings.json` retains the historical v7 inventory only; the active improved source is selected by `assets/render/selene/source-lock.json`. Meanwhile, `lunar-physics-fixtures.json` contains analytic checks for future simulation work.

The [UI integration contract](ui-integration.json) pins 21 project source files across the fixed integration base and the observed 0.18.4 design reference. It records current command/icon mappings, proposed groups, disclosure, quality recommendations and twelve acceptance cases. It registers no UI or commands.

JSON is authoritative for identifiers, counts and recipe values. Markdown explains policy and provides a readable index. Python scripts are optional development tooling for authoring and validation; they are not an application runtime or a proposed replacement for SolidCraft's Rust implementation.

Run the package validator from the repository root:

```sh
python3 docs/render-materials/validate_catalog.py
```

It checks data and specification consistency. It does not execute the render engine, measure a material, simulate granular mechanics or establish photorealism.

## Identity and scope

Existing `MAT-001`–`MAT-200`, `LUZ-001`–`LUZ-200` and `ENV-001`–`ENV-200` retain their semantic identity from the earlier planning inventory. The `LUZ` prefix is retained as a stable identifier; all names, descriptions, documentation and UI wording in this package are English. New material families occupy `MAT-201`–`MAT-600`; lighting and environments extend through `280`.

P0 means foundational or first-pack candidate, P1 means expansion after the foundation, and P2 means specialized or later work. Priority is a planning judgment, not availability. Low/Medium/High cost is a relative authoring/rendering estimate, not a hardware benchmark. Dependency lists express minimum intended capabilities and must be refined when a recipe is bound to a concrete executor.

The first shipped pack should be smaller than the full inventory: select approximately 40–60 verified appearances, 20–30 rigs and 20–30 environments, then expand by validated family. The catalog target of 1,160 content concepts must never be presented as 1,160 production-ready assets.

## Design decisions

- Keep **engineering material**, **render appearance**, and **simulation/contact properties** independent and explicitly linked when appropriate.
- Preserve real CAD dimensions in millimeters, angles in radians and Z-up orientation. Convert once at a documented SI physics boundary.
- Use one scene description across preview, final output and automation, with visible differences when an executor supports less.
- Give each lighting contribution one owner. A luminous surface and a sampling proxy must not emit twice.
- Save recipes as versioned resources, instance overrides and an immutable resolved render request.
- Prefer procedural, original assets for the foundation. AI can propose source maps or backgrounds; each output still needs provenance, material reconstruction and validation.
- Build the SELENE still before the animation module. A strong still provides a material, geometry, color and light reference for later frames.

## Repository placement

The requested integration base is `0xlillith/solidcraft`, branch `lab/render-materials`, inspected at `93a2922c5ee272fb605703ae84c05825b4a74607`. The catalog work is isolated in `docs/render-catalog-selene`, tracked by [issue #2](https://github.com/0xlillith/solidcraft/issues/2) and [PR #3](https://github.com/0xlillith/solidcraft/pull/3). The native SELENE continuation is isolated in `showcase/selene-functional`, based on catalog commit `10ada1b45a4eb285ce752ac72f9aaa968ff8b5c4`.

The implementation already under way in the other thread remains the authority for runtime behavior. Resolve this specification against the code that actually lands there; do not infer support from the existence of a catalog row.

## Prepared resource continuation

The [SELENE package](../../assets/render/selene/README.md) contains original numerical appearance recipes, reviewable scene configuration and the current native reconstruction workflow. Its opaque PBR projections can be inspected through the supplied swatch command script. Advanced optics, terrain generators and final-quality output remain gated on actual executor support and validation.

The owner supplied the improved [SELENE-showcase.solidcraft](../../assets/render/selene/source/SELENE-showcase.solidcraft). The [source lock](../../assets/render/selene/source-lock.json) is now `pinned_native`; [SOURCE.md](../../assets/render/selene/SOURCE.md) records its exact hash, author provenance, retained notices and runtime compatibility. The current native work adds [mechanism improvements](../../assets/render/selene/MECHANISMS.md) and a [Raspberry Pi 5 visual reconstruction](../../assets/render/selene/ELECTRONICS.md) through reproducible SolidCraft commands. The [native validation record](../../assets/render/selene/VALIDATION.md) identifies the saved derivative, executor, completed checks and remaining limits. The broader catalog remains unbound, and historical v7 remains excluded as a substitute.
