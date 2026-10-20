# Attribution

Every non-code asset in this repository (images, icons, fonts, example parts) is listed here with
its author, source and licence. `cargo xtask assets` (part of `cargo xtask ci`) fails if an asset
file is missing from this table.

**Policy (mandatory):** SolidCraft contains **no Autodesk iconography, images, artwork, fonts,
templates, materials or help text.** Every asset is original work by SolidCraft contributors or
third-party material under an open licence. Screenshots of Autodesk software are never committed.

Editable drawing code, native projects and shipped exports are collected under `assets/`.

## Files

| File | Author | Source | Licence |
|---|---|---|---|
| assets/render/catalog/capabilities.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/capabilities.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| assets/render/catalog/environments.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/environments.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| assets/render/catalog/lighting.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/lighting.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| assets/render/catalog/materials.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/materials.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| assets/render/catalog/quality-presets.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/quality-presets.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| assets/render/catalog/scene-presets.json | SolidCraft contributors | Byte-identical runtime export of docs/render-materials/scene-presets.json; generated and checked by tools/render-library/generate.py | MIT OR Apache-2.0 |
| examples/chebyshev-motion-trace.json | SolidCraft contributors | Original parametric Chebyshev lambda mechanism and associative-trace demo, constructed from analytic circle intersections; classical link proportions documented at https://doi.org/10.1002/rcs.70095; no external CAD geometry or artwork | MIT OR Apache-2.0 |
| examples/material-card.json | SolidCraft contributors | original analytic box and explicitly synthetic engineering card; invented illustrative constants, no external material dataset | MIT OR Apache-2.0 |
| examples/studio/author_sweep.py | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/author_scene.py | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/materials.json | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-exploded-sweep.json | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-exploded-render.json | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-exploded.json | SolidCraft contributors | original editable CAD scene and authoring metadata; no external assets | MIT OR Apache-2.0 |
| examples/studio/stage1-sweep.json | SolidCraft contributors | original tangent circular-arc CAD cyclorama; no external assets | MIT OR Apache-2.0 |
| examples/studio/stage1.json | SolidCraft contributors | original editable CAD studio scene; no imported geometry or external image assets | MIT OR Apache-2.0 |
| examples/studio/stage1-render.json | SolidCraft contributors | original studio camera and lighting parameters | MIT OR Apache-2.0 |
| examples/render-studio.json | SolidCraft contributors | original procedural box/cylinder enclosure script for the integrated Render workspace | MIT OR Apache-2.0 |
| examples/render-opaque-studio.json | SolidCraft contributors | original procedural sphere study varying opaque metal fraction and roughness independently; no external assets or engineering data | MIT OR Apache-2.0 |
| crates/gpu/src/path/denoise.wgsl | SolidCraft contributors | original bounded edge-aware a-trous spatial filter; GPU2 donor d710e31, no external shader source | MIT OR Apache-2.0 |
| crates/gpu/src/path/denoise_guides.wgsl | SolidCraft contributors | original deterministic centre-ray material and geometry guide pass; GPU2 donor d710e31, no external shader source | MIT OR Apache-2.0 |
| crates/gpu/src/path/shadows.wgsl | SolidCraft contributors | original bounded area, sun and environment visibility transport; GPU2 donor d710e31, no external shader source | MIT OR Apache-2.0 |
| crates/gpu/src/path/textures.wgsl | SolidCraft contributors | original planar colour and normal map sampling; GPU2 donor d710e31, no external shader source | MIT OR Apache-2.0 |
| assets/appearances/starter_pack.rs | SolidCraft contributors | original illustrative optical recipes, authored scalar values and future procedural resource descriptors; no measured or external material dataset | MIT OR Apache-2.0 |
| examples/material-finishes.json | SolidCraft contributors | original procedural sphere study of six illustrative opaque finishes with generic material densities; no external assets or measured optical data | MIT OR Apache-2.0 |
| examples/render-path-studio.json | SolidCraft contributors | original procedural opaque enclosure, teal insert and metal fasteners for progressive rendering; no imported models | MIT OR Apache-2.0 |
| crates/gpu/src/path/path.wgsl | SolidCraft contributors | original software BVH traversal and path integration using the existing SolidCraft surface/studio equations; no external shader source | MIT OR Apache-2.0 |
| crates/gpu/src/pbr.rs | SolidCraft contributors | original WGSL translation of SolidCraft's opaque surface and fixed studio reference, embedded as Rust source; no external shader code | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/icon.json | SolidCraft contributors | approved Icon Composer composition and appearance settings for the original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/02-grid.svg | SolidCraft contributors | original perspective grid with elliptical radial alpha, exported from VectorCraft | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/02-grid-dark.svg | SolidCraft contributors | original blue perspective grid with elliptical radial alpha, exported from VectorCraft for the Dark appearance | MIT OR Apache-2.0 |
| assets/icons/app/macos/source/SolidCraft.icon/Assets/03-solid-blue-cutout.svg | SolidCraft contributors | original saturated blue CAD solid with transparent topology cutouts, edited and exported with VectorCraft | MIT OR Apache-2.0 |
| assets/icons/app/macos/exports/Assets.car | SolidCraft contributors | Apple actool compilation of assets/icons/app/macos/source/SolidCraft.icon for macOS; contains original artwork only | MIT OR Apache-2.0 |
| assets/archive/icons/macos-v1/source/icon.vectorcraft | SolidCraft contributors | retained editable historical VectorCraft source, with named face and background layers; redundant exports retired | MIT OR Apache-2.0 |
| assets/icons/app/macos/exports/icon.icns | SolidCraft contributors | macOS fallback compiled by Apple's actool from the approved assets/icons/app/macos/source/SolidCraft.icon composition | MIT OR Apache-2.0 |
| assets/models/test_boat.bin | 3DBenchy.com | 3DBenchy.com | Public Domain |
| assets/models/make_test_boat.py | SolidCraft contributors | original script that packs a model into the compact file above | MIT OR Apache-2.0 |
| docs/screenshots/class-a-blend.png | SolidCraft contributors | screenshot of SolidCraft itself: two boxes bridged by `surface.blend` (a 40x10 sheet, `solidcraft-cli snapshot` of plan/evidence script ev_blend) | MIT OR Apache-2.0 |
| docs/screenshots/class-a-crown.png | SolidCraft contributors | screenshot of SolidCraft itself: a patch crowned with `surface.patch crown=0.3` over a saddle rim (`solidcraft-cli snapshot`) | MIT OR Apache-2.0 |
| docs/screenshots/class-a-boundary.png | SolidCraft contributors | screenshot of SolidCraft itself: a patch through two sketch lines built by `surface.boundary` with `crown: 0.4` (`solidcraft-cli snapshot`) | MIT OR Apache-2.0 |
| docs/screenshots/sample-plate.png | SolidCraft contributors | screenshot of SolidCraft itself (`solidcraft --sample`) | MIT OR Apache-2.0 |
| assets/samples/exports/0.png | SolidCraft contributors | original Sample Plate from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/1.png | SolidCraft contributors | original Sheet Metal Bracket from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/2.png | SolidCraft contributors | original Hinge Assembly from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/3.png | SolidCraft contributors | original Plastic Enclosure from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/4.png | SolidCraft contributors | original Bolted Flange from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/5.png | SolidCraft contributors | original Vent Plate from `crates/engine/src/sample.rs`, exported by the SolidCraft thumbnail renderer | MIT OR Apache-2.0 |
| assets/samples/exports/scripts.json | SolidCraft contributors | SHA-256 fingerprints of the original sample scripts, generated with the shipped thumbnails | MIT OR Apache-2.0 |
| docs/screenshots/demo-0.8.gif | SolidCraft contributors | original screen recording of the actual SolidCraft 0.8 English desktop UI modelling an original Raspberry Pi 4 case (no logos or branding; the engraved text is set in the system's DejaVu Sans Bold), generated with examples/demo/record_ui.sh and its original animated pointer overlay (9× playback) | MIT OR Apache-2.0 |
| docs/screenshots/demo-0.8.mp4 | SolidCraft contributors | original screen recording of the actual SolidCraft 0.8 English desktop UI modelling an original Raspberry Pi 4 case (no logos or branding; the engraved text is set in the system's DejaVu Sans Bold), generated with examples/demo/record_ui.sh and its original animated pointer overlay (9× playback) | MIT OR Apache-2.0 |
| docs/screenshots/rover-build.gif | 0xlillith (model); SolidCraft contributors (recording) | screen recording of the actual SolidCraft desktop UI replaying the timeline of the SELENE rover (`Rover-recorded.solidcraft` from https://github.com/0xlillith/solidcraft-rover, an original model by 0xlillith built through SolidCraft MCP); the model's lettering follows Ubuntu Light outlines (Canonical Ltd., Ubuntu Font Licence 1.0, as noted in that repository); the emblem is the SolidCraft project icon (2× playback) | MIT OR Apache-2.0 |
| docs/screenshots/rover-build.mp4 | 0xlillith (model); SolidCraft contributors (recording) | screen recording of the actual SolidCraft desktop UI replaying the timeline of the SELENE rover (`Rover-recorded.solidcraft` from https://github.com/0xlillith/solidcraft-rover, an original model by 0xlillith built through SolidCraft MCP); the model's lettering follows Ubuntu Light outlines (Canonical Ltd., Ubuntu Font Licence 1.0, as noted in that repository); the emblem is the SolidCraft project icon | MIT OR Apache-2.0 |
| docs/design/ux-redesign/model-floating.png | SolidCraft contributors | original UX mockup (Model tab, floating layout) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/model-compact.png | SolidCraft contributors | original UX mockup (Model tab, compact docked layout) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/sketch.png | SolidCraft contributors | original UX mockup (Sketch tab) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/sheet-metal.png | SolidCraft contributors | original UX mockup (Sheet Metal contextual tab) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/start-page.png | SolidCraft contributors | original UX mockup (start page (File view)) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/workspaces.png | SolidCraft contributors | original UX mockup (workspace switcher) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| docs/design/ux-redesign/command-map.png | SolidCraft contributors | original UX mockup (ribbon command map) rendered from an HTML mockup that uses the original Technical Accent icon set; no third-party artwork, and not a screenshot of any other CAD product | MIT OR Apache-2.0 |
| assets/branding/web/source/og.png | SolidCraft contributors | original 1280 × 640 social preview card (the SolidCraft mark, name and tagline), served by the docs site for link previews | MIT OR Apache-2.0 |
| docs/book/src/generated/sample-plate.png | SolidCraft contributors | copy of docs/screenshots/sample-plate.png made by `cargo xtask book` (gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/bracket.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot examples/bracket.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/enclosure.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/enclosure.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/revolve.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/revolve.json`; gitignored) | MIT OR Apache-2.0 |
| docs/book/src/generated/sheet-metal.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/sheet-metal.json`; gitignored) | MIT OR Apache-2.0 |
| assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf | IBM Corp. (IBM Plex Mono) | https://github.com/IBM/plex (via google/fonts), unmodified; licence text in assets/fonts/ibm-plex-mono/OFL.txt; the interface monospace font | SIL OFL 1.1 |
| assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf | IBM Corp. (IBM Plex Sans, variable wdth and wght) | https://github.com/IBM/plex (via google/fonts), unmodified; licence text in assets/fonts/ibm-plex-sans/OFL.txt; the default interface font | SIL OFL 1.1 |
| docs/book/src/generated/tool-preview.png | SolidCraft contributors | rendered by `cargo xtask book` (`solidcraft-cli snapshot docs/book/scripts/tool-preview.json`; gitignored) | MIT OR Apache-2.0 |
| assets/fonts/liberation-sans/LiberationSans-Regular.ttf | Red Hat, Inc. (Liberation Fonts 2.x; digitized data Google) | https://github.com/liberationfonts (Debian fonts-liberation), unmodified; licence text in assets/fonts/liberation-sans/OFL.txt; used for sketch Text outlines | SIL OFL 1.1 |
| assets/fonts/biz-udp-gothic/BIZUDPGothic-Regular.ttf | The BIZ UDGothic Project Authors | https://github.com/googlefonts/morisawa-biz-ud-gothic (Google Fonts), unmodified; licence text in assets/fonts/biz-udp-gothic/OFL.txt; used for web UI Japanese font | SIL OFL 1.1 |
| tests/step-conformance/cone_point.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cone_point_vertex.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder_inch.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder_nurbs.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder_placed.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder_seam.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/cylinder_twice.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/dome.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/dome_nurbs.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/far_cylinder.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/grooved_shaft.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/plate_100_holes.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/sheet_disc.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/tiny_pin.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/torus_ring_nurbs.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/step-conformance/washer.step | step-conformance contributors | generated from formulas by https://github.com/dan335/step-conformance (corpus/generated, 96f30bb); import regression fixture | CC0-1.0 |
| tests/oracle/17-loft/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/17-loft/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/23-bracket-multifeature/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/23-bracket-multifeature/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/28-blind-pocket-fillets/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/28-blind-pocket-fillets/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/30-fillet-after-shell/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/30-fillet-after-shell/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/31-fillet-l-all-edges/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/31-fillet-l-all-edges/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/34-chamfer-then-fillet/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/34-chamfer-then-fillet/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/35-fillet-curved-curved/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/35-fillet-curved-curved/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/36-corner-blend-two-radii/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/36-corner-blend-two-radii/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/37-pyramid-apex-blend/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/37-pyramid-apex-blend/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/39-tee-pipe/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/39-tee-pipe/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/41-tube-radial-holes/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/41-tube-radial-holes/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/42-overlapping-patterns/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/42-overlapping-patterns/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/47-draft-curved-pocket/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/47-draft-curved-pocket/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/51-loft-rect-circle-offset/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/51-loft-rect-circle-offset/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/56-holes-on-curved-face/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/56-holes-on-curved-face/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/58-phone-stand/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/58-phone-stand/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/59-enclosure-base/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/59-enclosure-base/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/61-shaft-keyway/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/61-shaft-keyway/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/63-hinge-knuckle/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/63-hinge-knuckle/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| tests/oracle/65-sketch-gear-tooth/reference.stl | SolidCraft contributors | triangulation of our own recipe (tests/oracle/65-sketch-gear-tooth/recipe.json) built with FreeCAD as an external tool by tools/freecad-oracle | MIT OR Apache-2.0 |
| assets/appearances/library.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/appearances/opaque_finishes.rs | SolidCraft contributors | original illustrative opaque finish values; no external dataset, textures or measured engineering data | MIT OR Apache-2.0 |
| assets/fonts/biz-udp-gothic/OFL.txt | The BIZ UDGothic Project Authors | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/fonts/ibm-plex-mono/OFL.txt | IBM Corp. | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/fonts/ibm-plex-sans/OFL.txt | IBM Corp. | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/fonts/liberation-sans/OFL.txt | Red Hat, Inc.; Liberation Fonts contributors | Unmodified accompanying font licence | SIL OFL 1.1 |
| assets/icons/app/linux/exports/hicolor/128x128/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/16x16/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/24x24/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/256x256/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/32x32/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/48x48/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/512x512/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/linux/exports/hicolor/64x64/apps/ai.storyteller.solidcraft.png | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/shared/source/app_icon.rs | SolidCraft contributors | Original SolidCraft drawing code: the borderless mark, drawn from the faces and gradients of assets/icons/app/macos/source/SolidCraft.icon/Assets/03-solid-blue-cutout.svg | MIT OR Apache-2.0 |
| assets/icons/app/shared/source/windows_ico.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/app/windows/exports/solidcraft-executable.ico | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/app/windows/exports/solidcraft.ico | SolidCraft contributors | Generated from assets/icons/app/shared/source/app_icon.rs with the original CLI/ICO encoders | MIT OR Apache-2.0 |
| assets/icons/ui/source/glyphs.rs | SolidCraft / VectorCraft contributors | Original glyphs; caption geometry adapted from VectorCraft titlebar.rs (https://github.com/storytold/vectorcraft) | MIT OR Apache-2.0 |
| assets/icons/ui/source/markers.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/ui/source/navigation.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/icons/ui/source/pointer.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/cs.json | potuzai | Czech translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/da.json | SolidCraft contributors | Original Danish translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/de.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/en-GB.json | SolidCraft contributors | English (UK): the English catalog's messages whose spelling differs, in UK spelling | MIT OR Apache-2.0 |
| assets/locales/en.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/es.json | jawler | Spanish (Spain) translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/fr.json | rlartot | French translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/gl.json | Pablo López (pablopeza) | Galician translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/hu.json | SolidCraft contributors | Original Hungarian translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/id.json | SolidCraft contributors | Original Indonesian translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/it.json | SolidCraft contributors | Original Italian translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/ja.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/ko.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/ms.json | A.Hakim (a-hakim) | Malay translation of the English message catalog | MIT OR Apache-2.0 |
| assets/locales/nl.json | SolidCraft contributors | Original Dutch translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/pl.json | cyryllo | Polish translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/ru.json | Alex Pro (del000) | Russian translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/sv.json | SolidCraft contributors | Original Swedish translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/pt-BR.json | DuBearte | Brazilian Portuguese translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/ro.json | SolidCraft contributors | Original Romanian translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/locales/tr.json | macrulz54 | Turkish translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/uk.json | Igor Bronovskyi (BrunIF) | Ukrainian translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/vi.json | vandang890615 | Vietnamese translation of the English catalog, contributed upstream | MIT OR Apache-2.0 |
| assets/locales/zh.json | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/locales/zh-TW.json | SolidCraft contributors | Original Traditional Chinese (Taiwan) translation of SolidCraft's English catalog; no commercial CAD text used | MIT OR Apache-2.0 |
| assets/materials/library.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.desktop | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.metainfo.xml.in | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/linux/ai.storyteller.solidcraft.mime.xml | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/macos/Info.plist.in | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/macos/README.txt | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/windows/solidcraft.en-us.wxl | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/platforms/windows/solidcraft.wxs | PhotoCraft / SolidCraft contributors | PhotoCraft packaging templates (https://github.com/storytold/photocraft), adapted for SolidCraft | Apache-2.0 |
| assets/shaders/viewport.wgsl | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/styles/ui/palettes.rs | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-dark.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-dark-hard.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-dark-soft.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-light.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-light-hard.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/gruvbox-light-soft.json | SolidCraft contributors | original SolidCraft theme file mapping SolidCraft's UI colours to the Gruvbox palette's colour values (Gruvbox by Pavel Pertsev, https://github.com/morhetz/gruvbox, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/solidcraft-95.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/platinum-9.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/cupertino.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/green-phosphor.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/amber-phosphor.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/blueprint.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/one-dark.json | SolidCraft contributors | original SolidCraft theme file with colours adapted from the One Dark palette (Atom One Dark by GitHub Atom, https://github.com/atom/atom, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/tokyo-night.json | SolidCraft contributors | original SolidCraft theme file adapting the colours of a published palette (Tokyo Night by enkia, https://github.com/enkia/tokyo-night-vscode-theme, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/solarized-light.json | SolidCraft contributors | original SolidCraft theme file adapting the colours of a published palette (Solarized by Ethan Schoonover, https://github.com/altercation/solarized, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/nord.json | SolidCraft contributors | original SolidCraft theme file adapting the colours of a published palette (Nord by Sven Greb and the Nord contributors, https://github.com/nordtheme/nord, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/high-contrast.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/graphite.json | SolidCraft contributors | original SolidCraft theme file | MIT OR Apache-2.0 |
| assets/styles/ui/themes/nosferatu.json | SolidCraft contributors | original SolidCraft theme file adapting the colours of a published palette (Dracula by Zeno Rocha and the Dracula Theme contributors, https://github.com/dracula/dracula-theme, MIT) | MIT OR Apache-2.0 |
| assets/styles/ui/themes/catppuccin-mocha.json | SolidCraft contributors | original SolidCraft theme file adapting the colours of a published palette (Catppuccin by the Catppuccin organisation, https://github.com/catppuccin/catppuccin, MIT) | MIT OR Apache-2.0 |
| assets/styles/web/loading-logo.svg | SolidCraft contributors | Animation of the SolidCraft mark, built from the paths and gradients of assets/icons/app/macos/source/SolidCraft.icon/Assets/03-solid-blue-cutout.svg; original animation | MIT OR Apache-2.0 |
| assets/styles/web/loading.css | SolidCraft contributors | Original SolidCraft resource data or drawing code, collected without redesign | MIT OR Apache-2.0 |
| crates/io/src/native_cad/fixtures/block.FCStd | SolidCraft contributors | original 10 × 20 × 30 mm test box saved by FreeCAD 1.1.4 run as an external program (`make_block_fcstd.py` beside it); no third-party geometry | MIT OR Apache-2.0 |
| crates/io/src/native_cad/fixtures/block.3dm | SolidCraft contributors | original 10 × 20 × 30 mm test box written with McNeel's MIT-licensed rhino3dm Python package (`make_block_3dm.py` beside it); no third-party geometry | MIT OR Apache-2.0 |
| assets/icons/ui/design/aliases.toml | SolidCraft contributors | icon ids that share a master, original SolidCraft data | MIT OR Apache-2.0 |
| assets/icons/ui/design/palette.toml | SolidCraft contributors | icon colour roles (light and dark) and accents, original SolidCraft data | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/bug_report.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/chevron.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/chevron_right.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/close.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/disclosure_closed.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/disclosure_open.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/fit.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/folder.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/help.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/home.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/lookat.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/moon.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/nav_settings.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/orbit.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/plus.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/pan.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/perspective.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/redo.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/save.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings_keyboard.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings_monitor.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings_mouse.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings_palette.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/settings_plug.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/spin.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/style.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/undo.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/zoom.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/accessibility.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/activate.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/align.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/appearance.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/arc3.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/arc_center.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/arc_slot.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/arc_tangent.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/as_built.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/auto_constrain.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/bendtable.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/blend.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/blend_surface.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/body.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/boss.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/boundary.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/boundary_fill.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/box.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/break.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_coincident.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_collinear.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_concentric.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_equal.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_fix.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_hv.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_midpoint.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_parallel.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_perpendicular.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_polygon.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_smooth.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_symmetry.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/c_tangent.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/camera.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/canvas.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/capture.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/center_of_mass.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/centerline.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/chain.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/chamfer.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/checkpoint.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/circle.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/circle2.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/circle3.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/circle_tt.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/circle_ttt.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/coil.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/colored_components.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/colored_components_off.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/combine.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/component.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/compute.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/configuration_table.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/conic.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/constrainer.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/construction.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/convert_sheet.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/copy.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/curvature_comb.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/curvature_map.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/cylinder.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/decal.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/delete.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/dimension.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/dots.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/draft.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/draft_analysis.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/drive.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/dxf.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/edge_continuity.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/edit.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/ellipse.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/emboss.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/environment_map.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/explode.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/export.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/extend.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/extrude.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/eye.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/fastener.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/fillet.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/filter.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/finish.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/fit_section.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/flange.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/flat.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/fold.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/folder.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/graph.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/hem.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/hole.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/import.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/include.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/inspect.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/interference.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/intersect.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/intersection_curve.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/iso_analysis.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/iso_curve.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/joint.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/joint_origin.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/library.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/line.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/line_mid.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/lip.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/list.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/loft.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/material.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/measure.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/min_radius.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/mirror.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/motion_link.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/move.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/new.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/offset.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/offset_face.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/offset_surface.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/open.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/origin.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/params.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/paste.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/patch.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern_circ.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern_path.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern_rect.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/perforate.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pipe.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/silhouette.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/assistant.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/agent_link.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/offset_two_sides.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/offset_symmetric.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/import_tray.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/export_tray.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/send.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/stop.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/shield.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/status_recovery.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/frame_member.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/placeholder.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_angle.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plastic_assign.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plastic_rule.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/play.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/polygon.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/presspull.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/project.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/project_surface.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rect.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rect3.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rect_center.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rect_rounded.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/refold.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rename.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/render_image.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/render_scene.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/reorder.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rest.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/restore.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/reverse_normal.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/revert.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/revolve.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rib.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rigid_group.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rollback.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/ruled.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/scale.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/section.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/select.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sheet.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sheet_rules.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/shell.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch_chamfer.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch_fillet.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch_mirror.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/slot.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/snap.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sphere.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/spline.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/spline_cv.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/split_body.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/split_face.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/spun.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/star.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/status_error.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/status_warning.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/stitch.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/flat/suppress.svg | SolidCraft contributors | single-colour application/navigation icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/surface.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/svg.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sweep.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/text.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/thicken.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/thread.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/torus.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/trim.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/unfold.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/unstitch.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/untrim.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/update.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/zebra.svg | SolidCraft contributors | two-tone modelling icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/as_built.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/component.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/curvature_comb.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/interference.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/iso_analysis.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/joint.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/pattern.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/small/section.svg | SolidCraft contributors | small-slot icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/workspace/ws_animate.svg | SolidCraft contributors | workspace icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/polygon_circumscribed.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/polygon_edge.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/slot_overall.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/slot_center.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/arc_slot_center.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch_chamfer_angle.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/sketch_chamfer_two_distances.svg | SolidCraft contributors | original sketch variant icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/gear.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/rack_pinion.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/screw.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/cam.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/trace.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/agent_chat.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/normal.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/infinite.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern_flip.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/pattern_center.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/reach.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/cut.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/compare.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/web.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/replace_face.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/bounding_solid.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/delete_face.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/snap_fit.svg | SolidCraft contributors | original icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/contact_sets.svg | SolidCraft contributors | original contact icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/contact_all.svg | SolidCraft contributors | original contact icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/contact_off.svg | SolidCraft contributors | original contact icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/contact_new.svg | SolidCraft contributors | original contact icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis_through_cylinder.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis_normal_to_face.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis_two_planes.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis_two_points.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/axis_edge.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_vertex.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_two_edges.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_three_planes.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_center.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_edge_plane.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/point_along_path.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_midplane.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_two_edges.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_three_points.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_tangent.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_along_path.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/model/plane_perpendicular.svg | SolidCraft contributors | original construction icon master (64 grid) | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/workspace/ws_design.svg | SolidCraft contributors | workspace icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/workspace/ws_drawing.svg | SolidCraft contributors | workspace icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/svg/workspace/ws_render.svg | SolidCraft contributors | workspace icon master (64 grid), original SolidCraft artwork | MIT OR Apache-2.0 |
| assets/icons/ui/design/references.toml | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/author/bases.py | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/author/new_icon.py | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/review/index.html | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/review/guide.html | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/review/review.css | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/review/review.js | SolidCraft contributors | original icon authoring or review tool resource | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/icon_palette.py | SolidCraft contributors | Original shared semantic icon palette resolver for repository tools | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/icon_status.py | SolidCraft contributors | icon lint, status page or preview tool (standard-library Python), original SolidCraft code | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/lint_icons.py | SolidCraft contributors | icon lint, status page or preview tool (standard-library Python), original SolidCraft code | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/preview.py | SolidCraft contributors | icon lint, status page or preview tool (standard-library Python), original SolidCraft code | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/review_state.py | SolidCraft contributors | icon review state helper (standard-library Python), original SolidCraft code | MIT OR Apache-2.0 |
| assets/icons/ui/design/requested_icons.txt | SolidCraft contributors | Runtime icon requests collected by the headless menu audit, original SolidCraft data | MIT OR Apache-2.0 |
| assets/icons/ui/design/review.json | SolidCraft contributors | icon review state (accepted icons and notes), original SolidCraft data | MIT OR Apache-2.0 |
| assets/icons/ui/design/tools/author/lib.py | SolidCraft contributors | icon authoring script (standard-library Python), original SolidCraft code | MIT OR Apache-2.0 |

| docs/book/src/og.png | SolidCraft contributors | copy of assets/branding/web/source/og.png staged by cargo xtask book (gitignored) | MIT OR Apache-2.0 |
| docs/sponsors/blacksmith-ci-powered.png | Blacksmith (blacksmith.sh) | "CI powered by Blacksmith" sponsor banner supplied by Blacksmith for the sponsorship credit; used only to credit the sponsor, not part of the application | © Blacksmith, used with permission |

## Code adapted from sibling projects

| SolidCraft file | Adapted from | Licence |
|---|---|---|
| xtask/src/layers.rs | CADCraft `xtask/src/layers.rs` (dependency layering rules and tests) | MIT OR Apache-2.0 |
| apps/solidcraft/src/control_server.rs | CADCraft `apps/cadcraft/src/control_server.rs` (loopback JSON-lines server) | MIT OR Apache-2.0 |
| crates/ui-egui/src/lib.rs (screenshot queue, synthetic input) | CADCraft `crates/ui-egui/src/lib.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/control.rs (method dispatch and screenshot saving) | CADCraft `crates/ui-egui/src/control.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/gpu.rs (paint-callback structure) | CADCraft `crates/ui-egui/src/gpu.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/server.rs, crates/mcp/src/backend.rs (JSON-RPC framing, MCP lifecycle, control-port bridge) | CADCraft `crates/mcp/src/{server,backend}.rs` | MIT OR Apache-2.0 |
| crates/mcp/src/tools.rs (tool schema helpers and argument checking) | CADCraft `crates/mcp/src/tools.rs`, GridCraft `crates/mcp/src/tools.rs` | MIT OR Apache-2.0 |
| crates/io/src/threemf.rs (bounded zip part reading and writing) | GridCraft `crates/xlsx/src/package.rs` | MIT OR Apache-2.0 |
| crates/ui-egui/src/titlebar.rs (title bar drag, caption buttons, resize zones, their tests) and the window setup in apps/solidcraft/src/main.rs | VectorCraft `crates/ui-egui/src/titlebar.rs`, `apps/vectorcraft/src/main.rs` | MIT OR Apache-2.0 |
| apps/solidcraft/src/open_documents.rs (macOS: files opened from Finder and the Dock, its test) | VectorCraft `apps/vectorcraft/src/open_documents.rs` (https://github.com/storytold/vectorcraft) | MIT OR Apache-2.0 |
| LICENSE-MIT, LICENSE-APACHE (licence text) | CADCraft | — |
| .github/workflows/release.yml, .github/workflows/packaging-lint.yml and packaging/ (release pipeline: Linux AppImage/deb/rpm/Flatpak/tar.gz, FreeBSD, macOS dmg, Windows MSI/portable zip, web zip; AppRun, nfpm, Flatpak, WiX and Info.plist templates) | PhotoCraft (https://github.com/storytold/photocraft) `.github/workflows/{release,packaging-lint}.yml`, `packaging/` | Apache-2.0 |

## Incremental HDR codec adapter

`crates/render/src/lighting/hdr.rs` adapts the scanline/component/old-RLE decoding
state machines from the Radiance decoder in `image` 0.25.10
([source](https://docs.rs/crate/image/0.25.10/source/src/codecs/hdr/decoder.rs)),
MIT OR Apache-2.0. Metadata parsing still uses that library. The adapter makes row
work cooperative and adds checked run lengths, zero-run rejection, width validation,
header/input/radiance bounds and cancellation. Original licence texts are retained
in THIRD-PARTY-LICENSES.txt; no images or artwork from that project are copied.

## File-format crates

| Crate | Used for | Licence |
|---|---|---|
| gltf 1.4.1, gltf-json 1.4.1, gltf-derive 1.4.1 (https://github.com/gltf-rs/gltf) | glTF 2.0 import (crates/io/src/gltf_in.rs): the JSON document model and its reference checks; the GLB container, buffers and accessors are read by SolidCraft's own code | MIT OR Apache-2.0 |
| inflections 1.1.1 (build-time, through gltf-derive) | case conversion in gltf's derive macros | MIT |
| lazy_static 1.5.1 (through gltf-json) | | MIT OR Apache-2.0 |

The glTF test files are built by the tests themselves (crates/io/src/gltf_in_tests.rs,
crates/engine/src/tests/gltf_import.rs); no third-party models are committed.

## FreeCAD and Rhino import library

FreeCAD `.FCStd` and Rhino `.3dm` files are read with [cadmpeg](https://github.com/cadmpeg/cadmpeg)
0.6.3 (Apache-2.0), linked as the crates `cadmpeg-core`, `cadmpeg-ir`, `cadmpeg-registry`,
`cadmpeg-container`, `cadmpeg-codec-freecad`, `cadmpeg-codec-rhino` and `cadmpeg-codec-step`
in native and browser builds (`crates/io/src/native_cad.rs`). Only those open-format codecs are built: no
default features, so cadmpeg's codecs for commercial CAD formats and its ShapeManager/ACIS
codec are not compiled in. Its licence text is in THIRD-PARTY-LICENSES.txt; no code from it is
copied into SolidCraft.

## Recovered contributions

| SolidCraft file | Original contribution | Licence |
|---|---|---|
| crates/ui-egui/src/viewport.rs (`nearest_quarter_turn`, `axis_view_orbit`) | N3MI-DG's [PR #101](https://github.com/bherbruck/solidcraft/pull/101): [minimal TOP/BOTTOM turns](https://github.com/bherbruck/solidcraft/commit/c28e780f52490784ef5886f094b9d3f6d10c3e3f) and [drag-to-orbit](https://github.com/bherbruck/solidcraft/commit/f30b30be6ecc4e2e6544a851243137c01a354934), adapted to the axis gizmo | MIT OR Apache-2.0 |

## Vendored third-party code

| Directory | Source | Licence | Changes |
|---|---|---|---|
| vendor/truck-meshalgo | truck-meshalgo 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | meshing: bounded parameter searches, collapsed-boundary projection, subdivision and provisional fixed-grid previews (vendor/README.md) |
| vendor/truck-stepio | truck-stepio 0.3.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | STEP writer: intersection-curve surfaces written at unique ids (vendor/README.md) |
| vendor/truck-shapeops | truck-shapeops 0.4.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | robust meshing and nearest-parameter fallback in booleans; degenerate intersection-curve rejection; face and edge history of booleans; trace macro; corner choice at vertices several intersection curves end on; reuse of an operand's meshes (vendor/README.md) |
| vendor/truck-geometry | truck-geometry 0.5.0, https://github.com/ricosjp/truck (RICOS Co. Ltd.) | Apache-2.0 | intersection curves divided from their neighbours' surface parameters; surface second derivatives (vendor/README.md) |
| vendor/matext4cgmath | matext4cgmath 0.1.0, https://github.com/ricosjp/matext4cgmath (RICOS Co. Ltd.) | Apache-2.0 | root refinement stops after 100 Newton steps (vendor/README.md) |

## Vendored renderer compatibility fix

`vendor/egui-wgpu` is egui-wgpu 0.36.2 from https://github.com/emilk/egui
(MIT OR Apache-2.0; both original licence texts are included). Rust sources and
its original `egui.wgsl` shader are retained; no images or other artwork are copied.
SolidCraft's narrow patch selects a safe 1x MSAA path on the chosen OpenGL adapter
and shares that effective count with the surface attachments and CAD callbacks.
Vulkan, DirectX 12, Metal and WebGPU retain their requested multisampling.
A frame whose meshes the GPU refuses to stage (device lost or out of memory) is
skipped with a warning instead of panicking (#1469).
Paint callbacks are prepared only after a drawable is acquired, so a skipped
surface frame cannot strand cached meshes without their GPU initialization.

## Runtime studio lighting mathematics

`crates/gpu/src/path/path.wgsl` implements the power-heuristic estimator from the
mathematical description in [PBRT fourth edition, section 13.4](https://pbr-book.org/4ed/Light_Transport_I_Surface_Reflection/A_Better_Path_Tracer).
No PBRT code or assets were copied. `crates/render/src/job/lighting.rs` evaluates
[Krzysztof Narkowicz's published 2016 ACES rational fit](https://knarkowicz.wordpress.com/2016/01/06/aces-filmic-tone-mapping-curve/)
using its published coefficients (author licenses the fit under CC0/MIT).
These original implementations are under SolidCraft's MIT OR Apache-2.0 licence.

## Original coffee framing variant

| File | Author | Provenance | Licence |
|---|---|---|---|
| examples/studio/author_coffee_framing.py | SolidCraft contributors | original editable composition, camera and documentation; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-framing.json | SolidCraft contributors | original editable composition, camera and documentation; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-framing-sweep.json | SolidCraft contributors | original editable composition, camera and documentation; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-framing-render.json | SolidCraft contributors | original editable composition, camera and documentation; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-framing.md | SolidCraft contributors | original editable composition, camera and documentation; no external assets | MIT OR Apache-2.0 |

## Original cylindrical coffee housing variant

| File | Author | Provenance | Licence |
|---|---|---|---|
| examples/studio/author_coffee_cylinder.py | SolidCraft contributors | original parametric cylindrical CAD construction and editable command recipes; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-cylinder.json | SolidCraft contributors | original cylindrical housing on the original exploded coffee composition; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-cylinder-sweep.json | SolidCraft contributors | original cylindrical housing and continuous CAD cyclorama; no external assets | MIT OR Apache-2.0 |
| examples/studio/coffee-cylinder-render.json | SolidCraft contributors | original camera, studio lighting and optical settings retained from the framing variant | MIT OR Apache-2.0 |
| examples/studio/coffee-cylinder.md | SolidCraft contributors | original dimensional recipe, provenance and validation limitations | MIT OR Apache-2.0 |

## Runtime environment sampling

`crates/render/src/lighting/environment.rs` and the environment/sun portions of
`crates/gpu/src/path/path.wgsl` are original SolidCraft implementations of
solid-angle importance sampling and MIS, informed by the mathematical description
in [PBRT fourth edition, sections 12.5](https://pbr-book.org/4ed/Light_Sources/Infinite_Area_Lights)
and [13.4](https://pbr-book.org/4ed/Light_Transport_I_Surface_Reflection/A_Better_Path_Tracer).
The procedural gradient and inline RGBE test fixtures are original mathematical
colours, not downloaded imagery. No third-party environment images or shader code
are included. Licence: MIT OR Apache-2.0.

## Original tumbler showcase assets

The CAD room, tumblers, window opening, wood/weave patterns and RGBE sky below
are original SolidCraft contributor work. No third-party scene, photograph, HDRI,
texture, or AI-generated artwork was used. Python authoring sources are included.
All listed files use the repository MIT OR Apache-2.0 licence.

| File | Source / author | Use | License |
| --- | --- | --- | --- |
| examples/studio/author_showcase.py | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/author_tumblers.py | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/textures/ATTRIBUTION.md | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/textures/author_textures.py | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/textures/environment-manifest.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/textures/manifest.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-hdr-textures-blocked-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-hdr-textures-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-materials.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-sun-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-sun-textures-blocked-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-sun-textures-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-sun.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-textures-blocked-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers-textures-render.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |
| examples/studio/tumblers.json | Original SolidCraft contributors | Editable scene / runtime resource / reproducible authoring metadata | MIT OR Apache-2.0 |

The original studio generators produce local ignored PNG maps and an RGBE gradient.
Generated image bytes are not source assets; their authorship, licences and expected
hashes are documented under `examples/studio/textures`. Rendered evidence belongs
on the results branch. No photographs, scans or external image assets are included.

## Additional render and showcase resources

| File | Author | Source | Licence |
|---|---|---|---|
| assets/render/starter/tools/check_all.py | SolidCraft contributors | original read-only authored/native resource validation orchestration and evidence receipts | MIT OR Apache-2.0 |
| assets/render/starter/tools/test_check_all.py | SolidCraft contributors | original validation-runner regression fixtures and subprocess tests | MIT OR Apache-2.0 |
| assets/render/starter/tools/requirements.txt | SolidCraft contributors | pinned offline authoring tool requirements; no application dependency changes | MIT OR Apache-2.0 |
| crates/render/examples/validate_starter.rs | SolidCraft contributors | original native resource conformance checker using public SolidCraft decoding, normalization, sampling and mip APIs | MIT OR Apache-2.0 |
| .gitattributes | SolidCraft contributors | original canonical line-ending policy for the existing runtime icon-discovery manifest; no artwork or icon-assignment changes | MIT OR Apache-2.0 |
| assets/render/.gitattributes | SolidCraft contributors | original canonical line-ending policy for SHA-256-pinned renderer authoring resources | MIT OR Apache-2.0 |
| docs/render-materials/.gitattributes | SolidCraft contributors | original canonical line-ending policy for SHA-256-pinned renderer authoring resources | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/assembly-bench-soft.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/automotive-turntable.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/black-metal-strips.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/cnc-overhead.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/esd-bench-neutral.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/fdm-neutral-inspection.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/garage-door.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/hangar-skylight.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/home-workbench.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/makerspace-worklights.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/mechanical-bay.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/neutral-comparison.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/neutral-side-softbox.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/paint-inspection-bands.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/printed-part-photo.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/rework-offset-strips.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/robot-led-editorial.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/showroom-ribbons.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/spacecraft-clean-bay.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/sun-free-overcast.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/vacuum-black.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/warm-table-window.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/white-enveloping-studio.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/exports/woodshop-window.hdr | SolidCraft contributors | original analytic relative-radiance HDR export; recipe, linear-sRGB primaries and SHA-256 in assets/render/starter/environments/manifest.json; not measured photometry | MIT OR Apache-2.0 |
| assets/render/starter/environments/manifest.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/environments/source/recipes.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/lighting/exports/rigs.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/lighting/recipe.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-001/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-001/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-001/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-001/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-001/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-002/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-002/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-002/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-002/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-002/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-003/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-003/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-003/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-003/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-003/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-004/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-004/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-004/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-004/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-004/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-005/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-005/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-005/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-005/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-005/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-006/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-006/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-006/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-006/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-006/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-007/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-007/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-007/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-007/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-007/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-008/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-008/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-008/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-008/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-008/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-009/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-009/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-009/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-009/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-009/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-010/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-010/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-010/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-010/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-010/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-011/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-011/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-011/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-011/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-011/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-012/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-012/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-012/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-012/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-012/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-013/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-013/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-013/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-013/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-013/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-014/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-014/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-014/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-014/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-014/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-015/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-015/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-015/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-015/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-015/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-016/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-016/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-016/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-016/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-016/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-017/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-017/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-017/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-017/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-017/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-018/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-018/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-018/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-018/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-018/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-019/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-019/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-019/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-019/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-019/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-020/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-020/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-020/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-020/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-020/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-021/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-021/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-021/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-021/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-021/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-022/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-022/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-022/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-022/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-022/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-023/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-023/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-023/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-023/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-023/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-024/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-024/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-024/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-024/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-024/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-025/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-025/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-025/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-025/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-025/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-026/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-026/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-026/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-026/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-026/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-027/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-027/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-027/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-027/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-027/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-028/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-028/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-028/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-028/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-028/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-029/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-029/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-029/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-029/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-029/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-030/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-030/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-030/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-030/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-030/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-031/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-031/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-031/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-031/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-031/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-032/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-032/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-032/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-032/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-032/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-033/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-033/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-033/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-033/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-033/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-034/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-034/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-034/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-034/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-034/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-035/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-035/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-035/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-035/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-035/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-036/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-036/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-036/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-036/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-036/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-037/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-037/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-037/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-037/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-037/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-038/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-038/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-038/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-038/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-038/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-039/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-039/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-039/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-039/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-039/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-040/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-040/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-040/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-040/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-040/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-041/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-041/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-041/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-041/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-041/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-042/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-042/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-042/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-042/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-042/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-043/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-043/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-043/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-043/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-043/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-044/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-044/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-044/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-044/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-044/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-045/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-045/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-045/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-045/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-045/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-046/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-046/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-046/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-046/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-046/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-047/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-047/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-047/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-047/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-047/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-048/base_color.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-048/height.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-048/metallic.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-048/normal_open_gl.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/exports/SURF-048/roughness.png | SolidCraft contributors | original deterministic periodic PBR map export; recipe, role, scale and SHA-256 in assets/render/starter/materials/manifest.json; not measured material data | MIT OR Apache-2.0 |
| assets/render/starter/materials/manifest.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/materials/source/recipes.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/pack.json | SolidCraft contributors | original clean-room renderer resource recipe or manifest; authored/unbound scope and source receipts in assets/render/starter/README.md | MIT OR Apache-2.0 |
| assets/render/starter/tools/environments.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/lighting.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/materials.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/test_environments.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/test_lighting.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/test_materials.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/starter/tools/validate_pack.py | SolidCraft contributors | original clean-room offline resource authoring, numerical verification or validation tool; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/selene/licenses/UFL.txt | Canonical Ltd. | exact Ubuntu Font Licence 1.0 notice retained from https://github.com/0xlillith/solidcraft-rover/blob/b32437915372d32b71419dc8f5143fe22ff1e869/UFL.txt for the modelled Ubuntu Light lettering | Ubuntu Font Licence 1.0 |
| assets/render/selene/materials/appearance-pack.json | SolidCraft contributors | original clean-room English appearance authoring data: 48 illustrative opaque PBR projections with separate advanced target specifications; no measured dataset or external assets | MIT OR Apache-2.0 |
| assets/render/selene/materials/appearance-swatches.json | SolidCraft contributors | original analytic command script for 48 opaque box swatches and a support base, derived only from the adjacent original appearance pack; no external artwork or engineering properties | MIT OR Apache-2.0 |
| assets/render/selene/models/SELENE-functional.solidcraft | SolidCraft contributors (native enhancements); 0xlillith (original rover); SychPL / Mateusz Sych (supplied showcase assembly); Canonical Ltd. (lettering outlines) | native-command derivative of the unchanged author-supplied source, with original bearing, suspension, gripper, electronics and presentation work; source and final hashes in assets/render/selene/SOURCE.md and VALIDATION.md | Model and showcase contributions: MIT OR Apache-2.0 (explicit SychPL grant https://github.com/tangency-labs/solidcraft/pull/5494#issuecomment-6375788360); lettering: Ubuntu Font Licence 1.0, notice retained in assets/render/selene/licenses/UFL.txt |
| assets/render/selene/motion/interference-pairs.json | SolidCraft contributors | original authored exact-body pair selection for bounded native clearance checks on the functional SELENE derivative; not a generated collision result | MIT OR Apache-2.0 |
| assets/render/selene/motion/mechanism-acceptance.json | SolidCraft contributors | original kinematic acceptance criteria, inspected source-container bindings and analytic planning checks from cited public SolidCraft documentation and the admitted native source; derivative runtime acceptance is recorded separately | MIT OR Apache-2.0 |
| assets/render/selene/motion/preflight-commands.json | SolidCraft contributors | original read-only inventory script using existing project commands; no source-model selectors or state mutations | MIT OR Apache-2.0 |
| assets/render/selene/motion/sample-sequence.json | SolidCraft contributors | original future lunar sampling and sealed-vial dynamics state specification; no simulation or physical calibration result | MIT OR Apache-2.0 |
| assets/render/selene/scenes/roles.json | SolidCraft contributors | original semantic role and candidate-material authoring specification for the prepared SELENE demo; no native geometry or external assets bundled | MIT OR Apache-2.0 |
| assets/render/selene/scenes/shots.json | SolidCraft contributors | original shot, framing and staged-sequence authoring specification for the prepared SELENE demo; no render or simulation result claimed | MIT OR Apache-2.0 |
| assets/render/selene/scenes/stages.json | SolidCraft contributors | original stage composition and light-ownership authoring specification derived from the project render catalog; scientific context linked in scenes/README.md | MIT OR Apache-2.0 |
| assets/render/selene/source-lock.json | SolidCraft contributors | original source-admission metadata and public-reference inventory pinning the supplied improved SELENE native file; original-model and font notices preserved in SOURCE.md | MIT OR Apache-2.0 |
| assets/render/selene/source/SELENE-showcase.solidcraft | 0xlillith (original rover); SychPL / Mateusz Sych (showcase assembly and helpers); SolidCraft contributors (emblem); Canonical Ltd. (lettering outlines) | unchanged author-published native file from https://github.com/SychPL/solidcraft/blob/32ec1720447e3fcc6bb6cc53a63fb5b39cfa21b9/SELENE-showcase.solidcraft; source hash and license evidence in assets/render/selene/SOURCE.md | Model and showcase contributions: MIT OR Apache-2.0 (explicit SychPL grant https://github.com/tangency-labs/solidcraft/pull/5494#issuecomment-6375788360); lettering: Ubuntu Font Licence 1.0, notice retained in assets/render/selene/licenses/UFL.txt |
| assets/render/selene/tools/bearings.py | SolidCraft contributors | original native-command authoring of grooved bearings, rolling-element kinematics, clamp hubs and arm/head clearances; analytic geometry, no copied CAD source | MIT OR Apache-2.0 |
| assets/render/selene/tools/build.py | SolidCraft contributors | original Python development tool for pinned-source native command replay, staged checkpoints, command-trace and geometry health checks; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/selene/tools/electronics.py | SolidCraft contributors | original native-command Raspberry Pi 5 visual reconstruction using public manufacturer mechanical dimensions; original package envelopes and connector internals, no manufacturer CAD or artwork copied | MIT OR Apache-2.0 |
| assets/render/selene/tools/export_views.py | SolidCraft contributors | original native MCP preview export tool for three reusable showcase views; changes only transient visibility and cameras and verifies the source master hash | MIT OR Apache-2.0 |
| assets/render/selene/tools/finishes.py | SolidCraft contributors | original native physical-material proxy and opaque PBR assignment generator preserving the supplied rover palette | MIT OR Apache-2.0 |
| assets/render/selene/tools/mechanisms.py | SolidCraft contributors | original native-command gripper sleeves, pads, wheel ownership, joint limits and inspection studies with independent analytic suspension relation | MIT OR Apache-2.0 |
| assets/render/selene/tools/prepare.py | SolidCraft contributors | original Python development tool for bounded native-container inspection and reviewed opaque body-assignment preparation; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/selene/tools/presentation.py | SolidCraft contributors | original native-command browser organization, camera/render setups and closed sample-vial glass preparation | MIT OR Apache-2.0 |
| assets/render/selene/tools/suspension.py | SolidCraft contributors | original native-command trailing-arm suspension, telescoping coilover loops, plain bushings, carrier gussets and local motion clearances derived from the admitted rover geometry; independent analytic pin-distance relation, no copied external CAD | MIT OR Apache-2.0 |
| assets/render/selene/tools/validate_native.py | SolidCraft contributors | original bounded native MCP verification tool for sampled SELENE kinematics, independent analytic follower and world-coordinate checks, optional exact-name interference pairs and unchanged input hashes; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/selene/tools/validate_prepared.py | SolidCraft contributors | original Python development validator for prepared SELENE resources, source status, references, hashes and command projections; no application runtime dependency | MIT OR Apache-2.0 |
| assets/render/selene/tools/yaw_supports.py | SolidCraft contributors | original native-command plain yaw journals, flanged brass thrust bushings, recessed axial retaining pins and mounting-head clearances derived from the admitted rover geometry; no copied external CAD | MIT OR Apache-2.0 |
| assets/render/selene/ui/demo-flow.json | SolidCraft contributors | original progressive-disclosure UI integration specification for the existing Render workspace; no copied UI artwork or runtime registration | MIT OR Apache-2.0 |
| docs/render-materials/baseline-setup-commands.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/capabilities.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/environment-presets.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/environments.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/lighting.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/lunar-physics-fixtures.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/materials.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/provenance.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/quality-presets.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/roadmap.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/scene-presets.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/selene-bindings.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/simulation-requirements.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/studio-presets.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/ui-integration.json | SolidCraft contributors | original English UI integration specification and exact source-revision mappings from SolidCraft's own implemented shell, UI kit and accepted icons; no copied artwork or runtime registration | MIT OR Apache-2.0 |
| docs/render-materials/ux.json | SolidCraft contributors | original clean-room English catalog, preset or analytic planning data; external references documented in docs/render-materials/SOURCES.md; no external assets bundled | MIT OR Apache-2.0 |
| docs/render-materials/validate_catalog.py | SolidCraft contributors | original Python development validator for catalog references, preset composition and analytic specification fixtures; no application runtime dependency | MIT OR Apache-2.0 |

### Built-in render resource adapters (personal integration batch, 2026-10-22)

The original adapters in `crates/engine/src/cmd/render/library.rs`, the generated lookup `library_data.rs` and its generator `tools/render-library/generate.py` project the already attributed starter and SELENE resources without replacing their bytes or authoring manifests. The added scalar texture channels, explicit color/V conventions and immutable-source/GPU-payload sharing are original contributor work under MIT OR Apache-2.0. No external code, commercial CAD data or new third-party imagery was imported. Source metadata and the original authoring attribution remain authoritative; pending runtime acceptance is not a photometric or engineering qualification.

## Physical reference resource additions

| File | Author | Source | Licence |
|---|---|---|---|
| assets/render/physical/bindings.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/exports/coverage.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/exports/native-cards.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/profiles.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/quantities.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/sources.json | SolidCraft contributors | original physical-reference curation, associations or derived schema-1 export; per-fact public sources retained without relicensing their publications | MIT OR Apache-2.0 |
| assets/render/physical/tools/check_physical.py | SolidCraft contributors | original offline physical-reference validation and projection tool | MIT OR Apache-2.0 |
| assets/render/physical/tools/physical.py | SolidCraft contributors | original offline physical-reference validation and projection tool | MIT OR Apache-2.0 |
| assets/render/physical/tools/test_physical.py | SolidCraft contributors | original offline physical-reference validation and projection tool | MIT OR Apache-2.0 |
| assets/render/physical/tools/export_library.py | SolidCraft contributors | original explicit reference-library planner and native no-loss export verifier; no external code | MIT OR Apache-2.0 |
| assets/render/physical/tools/test_export_library.py | SolidCraft contributors | original regressions for native capability refusal, source identity and physical-card preservation | MIT OR Apache-2.0 |

| tools/render-library/selene_studio.py | SolidCraft contributors | original bounded native MCP studio orchestration with pinned editable master, explicit GPU policy and request/export receipts; no generated model or image substitutes | MIT OR Apache-2.0 |

| tools/render-library/selene_recipe.py | SolidCraft contributors | original full-detail SELENE material/light composition and analytic camera fit; unchanged pinned native master, no external imagery or measured optical dataset | MIT OR Apache-2.0 |
| tools/render-library/test_selene_recipe.py | SolidCraft contributors | original independent projection, scaling, nonmutation and input-refusal fixtures for the SELENE studio recipe | MIT OR Apache-2.0 |

| assets/render/selene/scenes/studio-hero.json | SolidCraft contributors | original executable render intent and verified provenance for the selected native SELENE studio showcase; authored RGB appearance and relative light units, not measured optics or simulation results | MIT OR Apache-2.0 |
| tools/render-library/publish_selene.py | SolidCraft contributors | original bounded native-frame publication checks for explicit visual review, source identity, resource hashes, PNG CRCs and scanline limits; copies completed native bytes without image synthesis or resampling | MIT OR Apache-2.0 |
| tools/render-library/test_publish_selene.py | SolidCraft contributors | original tiny analytic PNG fixtures for source-size, CRC, expansion, dimensions and explicit-publication-consent regressions | MIT OR Apache-2.0 |

| tools/render-library/package_selene.py | SolidCraft contributors | original byte-preserving packaging of the reviewed native SELENE PNG, public provenance and checksums; bounded resource/PNG verification, no fonts, executables or new render execution | MIT OR Apache-2.0 |
| .github/workflows/selene-showcase.yml | SolidCraft contributors | original read-only artifact-delivery workflow for the reviewed native SELENE export; existing pinned repository actions, no compilation, package installation, secret use or renderer claims | MIT OR Apache-2.0 |

| tools/render-library/verify_linear_master.py | SolidCraft contributors | original independent validation driver for native linear EXR output using the official OpenEXR Python reader as an external test oracle; no codec implementation copied or production dependency added | MIT OR Apache-2.0 |

## Aging, effects and backgrounds showcase, 2026-10-23

| File | Author | Source | Licence |
|---|---|---|---|
| tools/render-showcase/instrument.py | SolidCraft contributors | original native-scene authoring, checksum-verified editorial assembly or bounded delivery tool; no third-party source or font binary copied | MIT OR Apache-2.0 |
| tools/render-showcase/compose.py | SolidCraft contributors | original native-scene authoring, checksum-verified editorial assembly or bounded delivery tool; no third-party source or font binary copied | MIT OR Apache-2.0 |
| tools/render-showcase/package.py | SolidCraft contributors | original native-scene authoring, checksum-verified editorial assembly or bounded delivery tool; no third-party source or font binary copied | MIT OR Apache-2.0 |

| distribution/SolidCraft-Render-Showcase.zip | SolidCraft contributors and the explicitly credited original model/UI/resource authors | byte-verified selected collaborator package of reviewed native media, editable sample, recipes and public provenance; contained attribution and existing SELENE lettering notices retained; no fonts or executable binaries | MIT OR Apache-2.0 for new contributions; referenced original notices retained |
| distribution/SolidCraft-Render-Showcase.json | SolidCraft contributors | original verified delivery archive manifest with exact SHA-256, member count and bounded scope | MIT OR Apache-2.0 |
