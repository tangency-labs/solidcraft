---
name: solidcraft-sheetmetal
description: Use when building or changing a sheet metal part in SolidCraft through its MCP server (flange, segments, hem, fold, unfold, flat pattern, DXF, bend table), or when a request describes bends in words.
---

# SolidCraft sheet metal

## Overview
A sheet metal part is a flat sheet plus bends: base first, then flanges. Work the numbers first (rule, flat size, stock, angles, heights, segment boundaries), build in `batch` calls, then check with `measure`, `sheet.flat_pattern` and `sheet.export_bend_table`. Contract, bend arithmetic, recipes: `reference.md`.

`segments` needs `feat/sheet-flange-segments`; `unfold_method` needs #9911.

## Before the first call
1. `skill_reference {"skill":"solidcraft-sheetmetal"}`; elsewhere read `reference.md`.
2. `inspect_design {"measure":false,"sketches":false}`; with timeline features, ask before `new_design` or `open`.
3. `sheet.rules` gives thickness, K-factor, radius, reliefs, gap. A flange takes none of them; `sheet.manage_rules` first when no rule fits.
4. Plan the flat size, bend fractions, flat area times thickness (the volume you check) and the stock.

## Build
- **Rule:** `sheet.manage_rules {"name":"AL2","thickness":"2 mm","k_factor":0.5,"bend_radius":"1 mm","active":true}`. Ask the shop for the K-factor; never invent it.
- **Base:** a closed sketch profile, then `sheet.flange {"sketch":"Base"}`.
- **Edge:** `sheet.flange {"type":"edge","edges":[...],"height":...,"angle":...}`; `{point, angle, height, segments}` gives one edge its own values.
- **Segments:** stretches `{from, to}` tiling the stretch the sheet still has, each a bend with its own angle, height and side; opposite sides are cut in the flat.
- **Chains:** `then` bends the far edge again; refused with `segments`, naming the edge.
- **Other:** `sheet.path_flange`, `sheet.hem`, `sheet.fold`, `sheet.convert`.
- Parameters, not numbers; edge picks are plain numbers.

## Verify
- `sheet.flat_pattern` and `sheet.export_bend_table {"format":"json"}` give the flat size, outline, each bend's centre line, length and allowance: check where a segment landed.
- `measure` must match the flat area times thickness; a bend that leaves the volume unchanged has not built.
- `inspect.interference`/`inspect.clearance` against mating bodies; `screenshot {"source":"model","view":"iso"}`.

## The AI workflow (FR-17 to FR-20)
1. **Segments:** derive count and boundaries from the intent (stiffeners, clearances, mating faces), state the assumption and confidence, offer an alternative, compute the fractions.
2. **Angles:** one angle labelled high (geometry), medium (shop standard) or low (inferred), plus an alternative; refuse when the geometry contradicts the words.
3. **Words to parameters:** map the phrase, read the numbers back when ambiguous. "90 degree flange 20 mm high on the long edge" is `sheet.flange {"type":"edge","edges":[[100,0,2]],"angle":"90 deg","height":"20 mm"}`.
4. **Optimisation:** at most 8 candidates over fractions, angles, heights and sides, scored by `inspect.interference` volume 0, `inspect.clearance` at least the gap, flat within stock, radii at or above the rule's. Stop at the objective; report the winner, its margin and each iteration.

## Red flags

| Thought | Do instead |
|---|---|
| "Assume the fraction direction" | Check the bend table, or keep the partition symmetric. |
| "Leave a gap between segments" | Segments tile the edge; a set-back is a different feature. |
| "It reported ok" | `measure` against flat area times thickness and the bend table. |
