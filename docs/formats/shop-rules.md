# Shop rules

A shop rules profile says what a fabrication shop asks of a sheet metal part: the sizes it
stocks, its smallest hole, how near a hole may be to an edge or a bend, whether threads and
hardware are modelled. `sheet.check_shop` holds a part against one. Each value keeps what it comes
from (a page, or a basis stated as text), so a number that changes is a one-line edit, and a rule
whose pages disagree says so.

SolidCraft ships one profile, `generic`, in `assets/sheet-metal/shop-rules.json`: general sheet-metal
design practice in our own words, not any shop's rules. Its distances are the rules of thumb, in
terms of the sheet thickness T and the inside bend radius R: a hole at least T wide, 2 T from an
edge and 2.5 T + R from a bend, and a flange at least 4 T long. It has no stock list or sheet size,
which differ from shop to shop. A shop's own numbers are not built in: they come from a plugin, or
from you, as a preset.

`sheet.check_shop` takes `shop` as `generic`, as the id of a preset, as the path of a profile file
(a path ends in `.json`), or as a profile object. Profiles are read only from files you name;
SolidCraft never fetches one.

## Presets

A preset is your own profile, kept under an id and a name and offered by `sheet.check_shop` and by
the Shop Check window beside Generic. It is checked exactly as any profile is (below) before it is
kept, and again when your list is read back, so a damaged entry is left out and never breaks the
others. Presets belong to you, not to a design: the app keeps them with your preferences (so they are
there in every design, and in the browser), and a script or the command line keeps the ones it saves
for the session.

- `sheet.shop_presets` lists the built-in profile and your presets (ids and names). With
  `{"profile": "generic"}` (or a preset's id) it also gives that profile as the object a preset starts
  from (Generic's has no `id`: the preset's is made from its name).
- `sheet.save_shop_preset` keeps a profile as a preset: `{"name": "Acme Fab", "profile": {...}}`, or
  `{"path": "acme.json"}` for a file holding one profile object (the format below). The id is the
  profile's `id`, or one made from the name (`Acme Fab` is `acme-fab`); a preset of the same id is
  replaced. A profile that is not valid is refused with the rule and the value. `Generic` is the
  built-in profile's name and id, and not a preset's. Up to 32 presets, each at most 256 KiB.
- `sheet.delete_shop_preset` removes one: `{"id": "acme-fab"}`.

In the Shop Check window the Shop row lists Generic and your presets. Under it, **Save as preset...**
keeps the shop shown (Generic's values for a first preset) under a new name, **Import profile
file...** reads a profile file into a preset, and **Delete preset** removes the one shown after a
second click. To set your shop's numbers, make a preset from Generic, or write a profile file in the
format below, then edit the values and import it again: the same id replaces the preset.

## File (version 1)

A profile given to `sheet.check_shop` as an object, or in a `.json` file whose path it is given,
is the profile alone:

```json
{ "id": "acme", "name": "Acme Fab", "note": "For a person reading the file.", "dxf_export": { ... }, "rules": [ ... ] }
```

(`id`, `note` and `dxf_export` are optional.) Its `id` is `custom` when it has none. `id` is lower
case letters, digits, `_` and `-`. A rule is given once; one a profile leaves out is listed as not
checked.

The versioned envelope is the separate format of the built-in catalogue,
`assets/sheet-metal/shop-rules.json`; `sheet.check_shop` does not read it from a path:

```json
{
  "format": "solidcraft-shop-rules",
  "version": 1,
  "note": "For a person reading the file.",
  "profiles": [ { "id": "acme", "name": "Acme Fab", "rules": [ ... ] } ]
}
```

## DXF export

`dxf_export` says how the shop wants its flat DXF exported, as the options of `sheet.export_dxf`:

```json
"dxf_export": {
  "layers": "cut", "bend_lines": true, "bend_notes": false, "hardware_notes": false,
  "sources": [ { "url": "https://...", "read": "2026-11-25", "says": "..." } ]
}
```

`layers` is `roles` (a layer for each role, the export's default), `single` (everything on layer 0,
bend lines too) or `cut` (every cut contour on layer 0, the bend lines and notes on their own
layers, the bend lines dashed). A field left out is the export's default; `sources` is required,
and `hardware_notes` is refused with `single`. A shop that wants one cut layer and the bend lines
recommends `cut`; one whose cutter would cut every line recommends `single` with
`"bend_lines": false`.

`sheet.check_shop` checks the DXF rules against this export when the call gives no `dxf` (the
export's defaults when the profile has none), and its reply's `dxf` says which options it used
and where they came from (`asked`, `recommended` or `default`). A `dxf` in the call overrides
it. The Shop Check window starts from it and shows it as "Export with: ...".

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

A **source** is `{"url"?, "read"?, "says"}`: an https page, the day it was read, and what it says in
our own words. A source holds no copied text. `url` and `read` may be left out for a basis that is
only text: `{"says": "general practice: ..."}`, which is how Generic states each rule; such a
source has no page for the Shop Check window to open.

A **limit** is `{"value" or "steps", "relative_to"?, "plus_radius"?, "short_value"?, "from"?,
"materials"?, "sources"}`:

- `value` is mm; with `"relative_to": "thickness"` it is a multiple of the sheet's thickness
  (`2` is 2 T). `plus_radius` adds a multiple of the nearest bend's inside radius (2 T + R is
  `value: 2`, `plus_radius: 1`).
- `steps` is a list of `{"up_to_mm", "value"}`, in order of `up_to_mm`, for a value that goes by
  thickness (a die width): the first step the sheet fits. A thicker sheet is past the table: the
  limit is not applied and the report lists the rule as not checked, never the last step carried on.
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
(`layers`, `bend_lines`, `bend_notes`, `hardware_notes`; when it has none, the profile's
`dxf_export`, else the export's defaults).
Each finding names the export option that fixes it. Letters in the flat are always outlines; only
the notes beside bends, beads and hardware are text entities. A DXF profile that restyles the layers
is not considered.

A **stock** entry is `{"materials"?, "thicknesses_mm" or "min_mm"/"max_mm", "sources"}`: a sheet is
stock when within 0.01 mm of a listed thickness, or in the range.

Everything is checked on reading: sizes, counts, text lengths, finite numbers in range, sorted
steps, https sources. What is wrong is refused with the rule and the value.
