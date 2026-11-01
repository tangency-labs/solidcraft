---
name: solidcraft-ground-rules
description: "Use before adding any dependency, icon, image, font, test reference data or product wording to SolidCraft, or when unsure whether something may be copied, read or named. Covers independence from other CAD products, GPL code, asset attribution and the product name."
---

# Independence, licences, assets and the name

The rules that cannot be broken in a PR. `AGENTS.md` lists them in one line each; the reasons are in CONTRIBUTING.md, "Rules that aren't negotiable".

## Independent
- Behaviour comes from public documentation, general CAD conventions and user feedback. File formats come from public specifications.
- Never copy another product's code, icons, artwork, help text, tooltips, templates or materials.
- Reference tests use analytic values (`tools/oracle-analytic/`) or open-source tools. Expected values never come from a commercial CAD product.
- Do not drive, script, automate or extract data from commercial CAD products (Fusion, SolidWorks, Onshape, ...) for this project: no test data, measurements, command lists, ids or files produced with them.
- Command ids are our own: dotted, lower-case `<area>.<thing>[.<variant>]`, never another product's id.

## No GPL, LGPL or AGPL code
- Do not read, copy or link OpenCASCADE, FreeCAD (including planegcs), SolveSpace, CadQuery/OCP, LibreCAD or QCAD. Do not even read their sources.
- `tools/freecad-oracle/` runs FreeCAD as a separate program through its public scripting API to compute reference values. Nothing from FreeCAD is linked, vendored or shipped. The sketch solver and the blend operations are our own.
- Permissive dependencies are fine: say which one you add and why in the PR. `THIRD-PARTY-LICENSES.txt` is regenerated on `main`; never edit it in a PR (`cargo xtask licences --check` only reports).
- Code adapted from other permissive projects (MIT OR Apache-2.0) is noted in ATTRIBUTION.md. Projects published by [storytold](https://github.com/storytold) can serve as references for implementation conventions.

## Assets: the most serious rule
- SolidCraft contains no iconography, images, fonts or artwork from commercial CAD products, ever. Screenshots of commercial CAD software are never committed.
- Every file asset is original or openly licensed and has a row in ATTRIBUTION.md. `cargo xtask assets` enforces it.
- Command icons are the original SVG masters in `assets/icons/ui/design/svg/`, compiled through `crates/ui-egui/src/icon_svg.rs`.
- Before any icon, icon assignment or media edit, read `assets/AGENTS.md`, `assets/icons/ui/AGENTS.md` and `assets/icons/ui/STYLE_GUIDE.md`. Use the canonical bases and named finished references there; never invent a parallel icon style. `cargo xtask icons` validates them.

## The name
- SolidCraft everywhere: crates `solidcraft-*`, binaries `solidcraft` and `solidcraft-cli`, `.sc` designs (`.solidcraft` still opens), the `solidcraft/N` format tag, `SOLIDCRAFT_*` environment variables, settings and recovery folders, the MCP server name, docs, CI and packaging.
- The old name survives only in the compatibility shims listed in `docs/compatibility.md`. Do not add new uses of it.
- Public docs, web and Flatpak deployments use project Pages `/solidcraft/`. Do not add permanent old-name Pages aliases.
- Labels, parameter docs, messages and docs use the terms in the glossary (`docs/book/src/reference/glossary.md`): one term for each idea. A change that adds a new idea adds its term there.
- Units are millimetres and radians internally; Z is up.
