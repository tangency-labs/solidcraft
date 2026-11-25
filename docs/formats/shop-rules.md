# Shop rules

A shop rules profile says what a fabrication shop asks of a sheet metal part: the sizes it
stocks, its smallest hole, how near a hole may be to an edge or a bend, whether threads and
hardware are modelled. `sheet.check_shop` holds a part against one. Each value keeps the page it
came from, so a number that changes is a one-line edit, and a rule whose pages disagree says so.

The built-in profiles are `assets/sheet-metal/shop-rules.json`: `generic` (what PCBWay, JLCCNC
and SendCutSend ask in common, taken strictly), `pcbway`, `jlccnc` and `sendcutsend`. Every value
there was checked against the shop's own page on 2026-11-25; a profile's note names the few
values that still need a spot check. `sheet.check_shop` also takes a profile as an object,
or the path of a file holding one (a path ends in `.json`). Profiles are read only from files you
name; SolidCraft never fetches one.

## File (version 1)

```json
{
  "format": "solidcraft-shop-rules",
  "version": 1,
  "note": "For a person reading the file.",
  "profiles": [ { "id": "acme", "name": "Acme Fab", "rules": [ ... ] } ]
}
```

A profile given to `sheet.check_shop` as an object, or in a file, is the profile alone
(`{"id", "name", "note"?, "rules"}`); its `id` is `custom` when it has none. `id` is lower case
letters, digits, `_` and `-`. A rule is given once; one a profile leaves out is listed as not
checked.

## Rules

A rule is `{"id", "unit"?, "process"?, ...}`; `unit` is `mm`, the only one, and `process` names
what the numbers are for (`laser cutting`) in the messages. The ids are a closed list.

| id | what it holds | data |
|---|---|---|
| `file.one_body` | one body in the file | `sources` |
| `file.uniform_thickness` | one thickness all over (no part-way recess) | `sources` |
| `stock.thickness` | the thicknesses the shop stocks | `stock` |
| `size.max_flat` | the largest flat pattern | `limits` |
| `hole.min_diameter` | the smallest round hole (and perforation hole) | `limits` |
| `hole.to_edge` | a hole's least distance to an edge, cut-out or other hole | `limits` |
| `hole.to_bend` | a hole's least distance to a bend | `limits` |
| `flange.min` | the shortest flange: the reach of each side of every bend | `limits` |
| `file.dxf_single_layer` | every cut contour of the flat DXF on one layer | `sources` |
| `file.dxf_closed_contours` | the flat DXF's contours closed, none cut twice | `sources` |
| `file.dxf_bend_lines` | the flat DXF's bend lines: `bend_lines`: `present` (a line at each bend) or `absent` | `bend_lines`, `sources` |
| `file.text_outlined` | text in the flat DXF as outlines, not text entities | `sources` |
| `thread.modelling` | `wanted`: `plain` (a plain hole, the thread on the order) or `modelled` | `wanted`, `sources` |
| `hardware.modelling` | `wanted`: `plain` (named on the order) or `modelled` | `wanted`, `sources` |

A **source** is `{"url", "read", "says"}`: an https page, the day it was read, and what it says in
our own words. A source holds no copied text.

A **limit** is `{"value" or "steps", "relative_to"?, "plus_radius"?, "short_value"?, "from"?,
"materials"?, "sources"}`:

- `value` is mm; with `"relative_to": "thickness"` it is a multiple of the sheet's thickness
  (`2` is 2 T). `plus_radius` adds a multiple of the nearest bend's inside radius (2 T + R is
  `value: 2`, `plus_radius: 1`).
- `steps` is a list of `{"up_to_mm", "value"}`, in order of `up_to_mm`, for a value that goes by
  thickness (a die width): the first step the sheet fits; a thicker sheet takes the last.
- `short_value` (`size.max_flat` only): the most the shorter side may be; `value` is the longer's.
- `from` (`hole.to_bend` and `flange.min`): `bend_start` (default: where the flat sheet ends and
  the bend begins, the most generous reading, so a finding is a real one) or `bend_line`.
- `materials`: parts of the sheet rule's material (ignoring case) the value is for. A limit
  without any is for every material. When none matches the sheet's material, every limit is used
  and the report says so.

Several limits in a rule all hold, and the strictest governs (the smallest sheet for
`size.max_flat`). A rule marked `"disagreement": true` is one whose pages differ: the strictest
value is used, and its findings are warnings instead of errors.

`flange.min` is held against the reach of each panel on each side of each bend: in the flat, the
least distance, square to the bend, from the edge of the bend's zone (or its centre line) across
the panel to the panel's far boundary, taken over the bend's length less one thickness at each end
(so a rounded or chamfered corner does not decide it). A hole inside the panel does not shorten it;
a cut-out through to the panel's edge does. A boundary of free-form curves is not measured and is
listed as not checked. A finding is at the bend's middle.

The four `file.dxf_*` and `file.text_outlined` rules are held against the flat DXF `sheet.export_dxf`
would write, read from the export's own layout, for the options `sheet.check_shop` is given in `dxf`
(`layers`, `bend_lines`, `bend_notes`, `hardware_notes`; the export's defaults when it has none).
Each finding names the export option that fixes it. Letters in the flat are always outlines; only
the notes beside bends, beads and hardware are text entities. A DXF profile that restyles the layers
is not considered.

A **stock** entry is `{"materials"?, "thicknesses_mm" or "min_mm"/"max_mm", "sources"}`: a sheet is
stock when within 0.01 mm of a listed thickness, or in the range.

Everything is checked on reading: sizes, counts, text lengths, finite numbers in range, sorted
steps, https sources. What is wrong is refused with the rule and the value.
