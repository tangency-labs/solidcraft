# Sheet metal rule packs

A rule pack is a small file of sheet metal rules that you or your fabrication shop can write to
match real stock and tooling. Import one from the Sheet Metal Rules manager (Standard library >
Import pack…): its rules join your personal rules, which every design's manager offers.
Export pack… writes your personal rules as a pack. Packs are read only from local files
you choose; SolidCraft never fetches one.

SolidCraft's standard library (`assets/sheet-metal/standard-library.json`) is itself a pack in
this format.

## JSON (version 1)

```json
{
  "format": "solidcraft-sheet-rules",
  "version": 1,
  "name": "Acme Fab press brake 2",
  "note": "Air bent on 1T punches; ask for anything thicker than 10 ga.",
  "sources": [
    {"id": "chart", "title": "Acme Fab bend chart, rev C", "url": "https://example.com/bend-chart.pdf"}
  ],
  "rules": [
    {
      "name": "CRS 16 ga",
      "material": "Mild steel (cold-rolled)",
      "gauge": "16 ga",
      "chart": "Acme Fab stock list",
      "hardness": "medium",
      "thickness": "0.0598 in",
      "k_factor": "0.42",
      "bend_radius": "0.8 * Thickness",
      "source": ["chart"]
    }
  ]
}
```

- `format` must be `solidcraft-sheet-rules`. `version` is a whole number; left out, it is 1.
- `name` and `note` describe the pack. `sources` lists where the values come from; a rule's
  `source` names their ids (one id or a list), and an id not in `sources` is refused.
- Each rule needs `name` and `thickness`. The thickness is the data: `gauge` is only a label,
  and `chart` says whose gauge chart it is from (different charts give one gauge different
  thicknesses).
- `material` labels the rule. `hardness` (`soft`, `medium` or `hard`) is the column of the
  generic K-factor chart the material reads, and `min_bend_radius` its published minimum inside
  radius (a length, may use `Thickness`).
- `k_factor` and `bend_radius` are a shop's own: a K-factor measured from test bends and the
  working radius of its tooling. A rule without them keeps the rule's own radius when picked,
  and its K-factor is estimated from the bend method (air bending, bottoming or coining) at that
  R/T, and said to be an estimate.
- A rule may also give `relief_width`, `relief_depth`, `corner_relief`, `corner_relief_shape`
  (`notch`, `round`, `rectangular` or `tear`), `hem_gap`, `gap` and `angle_display` (`bend` or
  `included`), the values of `sheet.manage_rules`. Imported into personal rules, what a rule
  leaves out takes SolidCraft's default rule's value.
- Values are expressions, as in the manager. Lengths carry their unit (`1.5 mm`, `0.0598 in`),
  since a pack goes into designs of any unit, and may use the rule's own `Thickness`
  (`0.8 * Thickness`). They may not use a design's parameters.
- Rule names are unique, ignoring case.

## CSV

A CSV pack holds the rules only: no name, note or sources. The first row names the columns,
with the JSON names (any case, any order); `name` and `thickness` are required.

```csv
name,material,gauge,chart,thickness,k_factor,bend_radius
CRS 16 ga,Mild steel (cold-rolled),16 ga,Acme Fab stock list,0.0598 in,0.42,0.8 * Thickness
AL 3 mm,Aluminium 5052-H32,,,3 mm,0.45,3 mm
```

Fields with commas or quotes are quoted as spreadsheets write them. A field SolidCraft writes
that a spreadsheet would read as a formula starts with `'`, which is taken off again on import.

## What is refused

A pack is checked before any rule of it is used, and a refused pack adds nothing. It is refused,
with a message naming the rule and the value, when it is over 1 MiB or 1000 rules, is not UTF-8,
is not valid JSON or has unmatched CSV quotes, lacks a required value, has a length with no
unit, a parameter definition (`T = 2 mm`), a parameter other than `Thickness`, a thickness
outside 0 to 10000 mm, a K-factor outside 0 to 1, a bend radius of 0 or less, text over its
length or with line breaks, an unknown corner relief shape or angle display, or two rules of one
name.

## Newer packs

A pack of a later version, or with fields or columns this version does not know, is read for
what this version knows, and the import names the fields it skipped. Importing never changes a
personal rule you already have: a pack rule with the same name is left out and named.
