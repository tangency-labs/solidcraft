# Generator packages

A generator turns a manufacturer's ordering code (for example `HNTT5-M4`) into a part. A generator is
**data, not code**: SolidCraft never runs a third-party program for it. It reads a small package,
parses the code with the package's grammar, looks up dimensions in the package's tables, fills a
command template with them and runs the commands through the normal command path (the same schemas
and checks as a script). Part of RFC 0017 (the Workshop); issue #22331.

Format tag: `solidcraft-generator/1`. This page is the format; the machine form is
[`generator-package.schema.json`](generator-package.schema.json), and a worked package is
`examples/generators/slot-nut-20.generator.json`.

## 1. File layout, version, minimum build

- A package is **one UTF-8 JSON file**, named `<id>.generator.json`, at most 1 MiB. Tables are
  CSV text inside it, so the file is the whole package and a Workshop item is that one file (or a
  zip holding it). There is no folder form in version 1.
- `format` is `solidcraft-generator/1`. A build that does not know the major number refuses the
  package with "needs a newer SolidCraft"; unknown keys at the top level are refused too (a typo
  must not pass silently), except under `x-` keys, which are kept and ignored.
- `min_solidcraft` is the oldest SolidCraft that can run the package (`"0.52.0"`). An older
  build refuses it and says which version it needs.
- `id` is lower-case letters, digits and dashes, 3 to 64 characters. `name`, `description`,
  `license` (an SPDX id) and `author` are text.

## 2. The ordering-code grammar

`grammar` has `alternatives`, a list of token lists; a code must match exactly one alternative
completely (the first that matches wins; a code that fits none is refused with the grammar's
`refuse` text). Matching is hand-written code that tries the longest fit first and backs up when a later token fails, within a budget of 20,000 matching steps (a code is at most 128 characters); an integer has no leading zeros. Tokens:

| Token | Meaning |
|---|---|
| `{"lit": "HNTT"}` | fixed text (case-insensitive when `"nocase": true`) |
| `{"field": "type", "enum": ["5","6"]}` | one of a list; the longest listed value that fits is taken. `"labels"` may map a value to a display text |
| `{"field": "length", "int": {"min": 20, "max": 4000, "step": 5}}` | an integer in a range, on a step (`step` default 1) |
| `{"field": "w", "decimal": {"min": 0.5, "max": 9, "step": 0.5, "digits": 1}}` | a decimal with at most `digits` places |
| `{"field": "id", "chars": "digits"\|"alpha"\|"alnum", "min_len": 1, "max_len": 8}` | a run of characters, for codes that are free-form |
| `{"optional": [tokens], "default": {"m": "5"}}` | tokens that may be left out; `default` gives the field values when they are |
| `{"flags": {"field": "options", "options": [{"code": "-C", "value": "coated"}], "max": 3}}` | optional suffixes, each at most once, in any order; the field is the list of values given |

Every field has a value after parsing, either typed from the code or from a `default`. A field is
text, a number, or (for `flags`) a list of text. The parse result is the **fields** object, shown in
the dialog.

**Cross-field constraints, refusals and defaults** are `rules`, run after parsing and after the
lookups of section 3, in order. A rule is

```json
{"when": "type == \"6\" && m != \"M5\"", "refuse": "HNTT{type} is offered in M3, M4, M5, not {m}", "kind": "invalid"}
```

`when` is a boolean expression (section 4). When it is true the code is refused with `refuse`
(`{name}` in the text is replaced by a field, lookup value or variable). `kind` is `invalid` (the
default: the code is not an ordering code of this series) or `not_modelled` (a real code this
generator does not model; the dialog says so in different words and the command reply says
`"reason": "not_modelled"`). Defaults are in the grammar (`default` above) and in `defaults`, a
`{field: expression}` object for values that depend on others (used only when the field is
absent). The order of evaluation is: grammar, `defaults`, `lookups`, `vars`, then `rules`.

**RFC 0019 regex declarations.** A Workshop manifest's `[[declares.matches]]` regex is a
*search hint*: it lets global search offer the generator for a typed string. It does not parse.
The package's own grammar is the authority, and the package may carry the same declaration:

```json
"declares": {"matches": [{"pattern": "^HNTT(?P<type>[56])(?:-(?P<m>M[3-6]))?$", "examples": ["HNTT5-M4"], "negative": ["HNTT7"]}]}
```

SolidCraft does not evaluate the regex (no regex engine runs on package data). The package check
(section 8) requires every `examples` code to parse and every `negative` to be refused, so a
pattern cannot claim a code the grammar refuses. Named groups map one to one to fields: a group
`(?P<m>M[3-6])` becomes `{"field":"m","enum":["M3","M4","M5","M6"]}`, a `\d+` becomes `int` with
`min`/`max`, and `(...)?` becomes an `optional` token with a `default`. A converter from a regex
form can therefore emit the grammar mechanically; a hand-written grammar does not need one.

## 3. Tables

```json
"tables": {
  "series": {
    "columns": {"type": "text", "E": "number", "T": "number", "allowed_m": "list", "polygon": "points",
                "source": "text", "page": "text", "status": "text"},
    "key": ["type"],
    "csv": "type,E,T,allowed_m,polygon,source,page,status\n5,10.0,4.0,M3 M4 M5,0 0;10 0;10 4,S1,12,published\n"
  }
}
```

- `csv` is comma-separated with a header row; a cell may be quoted with `"`; at most 5000 rows and
  64 columns. Column types: `text`, `number` (finite), `list` (space-separated texts),
  `points` (pairs `x y` separated by `;`, at most 512 points, finite). A cell that does not fit its
  type refuses the package at load, naming the row and column.
- `key` names the columns that identify a row; a repeated key refuses the package.
- Reserved provenance columns, all optional: `source` (an id from `sources`), `page`, `status` (one
  of `published`, `measured`, `derived`, `approximate`) and `note`. They are not dimensions, but the
  UI shows them as the part's provenance (section 6), and a `status` that is `approximate` is
  flagged there.
- `lookups` join rows to the parsed fields, by key, as named records:

```json
"lookups": [
  {"table": "series", "as": "s", "key": {"type": "type"}, "missing": "HNTT{type} is not in the table"},
  {"table": "tap", "as": "t", "key": {"m": "m"}}
]
```

  A later lookup may use an earlier one's columns as its key (`"key": {"m": "s.default_m"}`). A
  record's columns are read as `s.E`, `t.drill`. A missing row refuses the code with `missing`.
  Lookup by a field (type to E, T, allowed M) and by a second field (M to tap drill) are both just
  lookups; a list column is tested with `in(m, s.allowed_m)`, so "offered in M3, M4, M5, not M6" is
  `{"when": "!in(m, s.allowed_m)", "refuse": "HNTT{type} is offered in {s.allowed_m}, not {m}"}`
  (a list shows as its items joined by `, `).

## 4. Expressions

A tiny pure language, evaluated by SolidCraft's own evaluator (no I/O, no loops, no calls out, no
recursion). Values are numbers (finite, `f64`), text, booleans and lists.

- Numbers: `12`, `0.5`, `1e-3`. Text: `"M4"`. References: a field, lookup record column
  (`s.E`), variable, or a `repeat` index.
- Operators: `+ - * / %` (`+` also joins text), unary `-` and `!`, `== != < <= > >=`, `&& ||`
  (short circuit), `c ? a : b`, parentheses.
- Functions: `min max abs round floor ceil sqrt pow`, `sin cos tan asin acos atan atan2` (degrees),
  `pi()`, `if(c, a, b)`, `in(x, list)`, `len(list)`, `at(list, i)`, `join(list, sep)`,
  `fmt(x, digits)` (number to text), `concat(a, b, ...)`, `upper lower`.
- Division by zero, a non-finite result, `sqrt` of a negative, an unknown name, a wrong type or an
  out-of-range index is an error that **refuses the code** with the expression and the cause (never
  a panic, never `NaN` in a command).
- Bounds: an expression is at most 512 characters and 256 parts, and nests at most 32 levels
  (each parenthesis, call argument, sign and `?` counts one level, a sign or `!` and a
  parenthesis each count two); a whole build costs at most 1,000,000 evaluation steps.

**Variables.** `vars` is an ordered list `{"name": "neck", "expr": "s.E / 2"}`; a variable may use
fields, lookups and earlier variables only, so there are no cycles (a forward reference is an
error).

## 5. The command template

`steps` is the list of commands to run, in order, each as the script/command JSON already used by
`solidcraft-cli run`:

```json
{"command": "solid.box", "params": {"length": "${s.E}", "width": "${s.E / 2 + 1}", "height": "${s.T}", "name": "${fmt(s.E, 1)} slot"}}
```

- In `params` (at any depth) a string that is exactly `${expr}` becomes the expression's value
  (a number stays a number, a list a JSON array, text text); a string with `${...}` inside it
  becomes text with each expression formatted. `$${` writes a literal `${`.
- **A polygon from a table** is `"points": "${s.polygon}"`: a `points` cell becomes
  `[[x, y], ...]`, as many points as the row has. A sketch command that takes a list of points
  takes it that way, so one table row can hold a whole profile.
- **Optional features** are a step (or a whole block) with `"when": "expr"`: it runs only when the
  expression is true (for example `"when": "in(\"-C\", options)"` for an option suffix).
- **Repeats** are `"repeat": {"var": "i", "count": "n_holes", "max": 64}` on a step: the step runs
  for `i = 0 .. count - 1`, with `i` usable in its expressions (`"x": "${edge + i * pitch}"`). The count
  is an expression, evaluated once; it must be a whole number from 0 to `max` (`max` is at most
  1024), else the code is refused. A build expands at most 512 steps in all.
- Only commands in the **allowed list** may be used: model-building commands (sketch, solid,
  component, parameter and feature commands). File, plugin, network, settings, shell and
  generator commands are refused when the package loads, as are commands that open dialogs.
- Each step runs through `Session::execute`, so the command's own schema check, undo and errors
  apply. The whole build is one undo step: if any step fails, the design is left as it was, and the
  error names the failing step and command.
- `insert` says where the part goes: `{"component": "${code}"}` (the default) makes a new component
  named after the code and builds the steps inside it.

## 6. Provenance, sources and the guarantee level

```json
"sources": [{"id": "S1", "title": "Series catalog, 20-series nut page", "url": "https://example.com/catalog.pdf", "pages": "12-13"}],
"provenance": "Dimensions are read from the public catalog sheet; the polygons are redrawn by hand.",
"guarantee": "checked"
```

- `sources` lists documents (id, title, optional url and pages); table rows point at them with
  `source`/`page`.
- `provenance` is text the UI shows verbatim beside the part (at most 2000 characters).
- `guarantee` is `checked` (the package's own `checks` pass), `reviewed` (plus a named independent
  reviewer) or `drawing-matched` (plus a match against the maker's drawing). **A package's own
  claim is only a label.** The Workshop index build verifies it and sets the badge a user sees
  (RFC 0017, RFC 0019); a package installed from a file shows "declared" until then. SolidCraft
  itself checks `checked` by running the package's `checks`.
- The part's provenance in the UI is: the provenance text, the sources it used, and the rows used
  with their `status`.

## 7. Checks the package carries

```json
"checks": [
  {"code": "HNTT5-M4", "volume_mm3": 412.3, "rel_tol": 0.001, "bbox_mm": [10.0, 4.0, 10.0], "faces": 14, "dimensions": {"thread_hole_d": 4.0}},
  {"code": "HNTT6-M6", "invalid": true, "message_contains": "not M6"}
]
```

A positive check builds the part and compares what is given: the volume (relative tolerance
`rel_tol`, default 1e-6), the bounding box size (`abs_tol` mm, default 1e-6), the face count, and
named `dimensions` (the values of variables, exactly). An `invalid` check requires the code to be
refused and the refusal text to contain `message_contains`. `generator.check` runs them all and
reports each; the same list is what the Workshop runs at publish time.

## 8. Commands

| Command | Does |
|---|---|
| `generator.parse` | Parses a code against a package (no design change). Reply: the fields, the lookup rows used with their provenance, the planned step count, or the refusal and its kind |
| `generator.build` | Parses, then builds the part into the open design. Reply: the part's feature ids, volume, bounding box, face count and provenance |
| `generator.check` | Runs the package's `checks` and reports each |
| `generator.list` | Lists installed packages |

`package` is either the path of a package file or the `id` of an installed one. No GUI is needed:

```
solidcraft-cli run -- generator.build '{"package": "slot-nut-20.generator.json", "code": "HNTT5-M4"}'
```

`generator.build` returns `volume_mm3`, `bbox_mm` and `faces`, and with `"save": "path.sc"` also
saves the design, so a maker's own build can be compared number for number.

## 9. Limits and hostile input (the format stays data)

A package and a code are untrusted. The format is not a programming language: it has no
recursion, no user-defined functions, no unbounded loops, no I/O and no clock. Every bound below
is enforced with a typed error (`GeneratorError`) that names the limit and what exceeded it. Time
is bounded by counting evaluation steps, not by a wall clock, so a limit behaves the same on every
machine and in the browser.

| Limit | Value | Checked |
|---|---|---|
| Package file | 1 MiB | before parsing |
| Ordering code | 128 characters | before parsing |
| Grammar matching | 20,000 steps per code; optional groups nest at most 4 deep | during matching, at load |
| Text value (any one string, input or result) | 4096 characters | on creation |
| Table | 5000 rows, 64 columns | at load |
| Points in one cell | 512 | at load |
| Expression | 512 characters, 256 parts, nesting 32 | at load |
| Whole build | 1,000,000 evaluation steps | at evaluation |
| Steps in the package | 256 | at load |
| Repeat count | a whole number, 0 to its `max`; `max` at most 1024 | after evaluating the count and **before expanding** |
| Expanded commands (all repeats together) | 512 | before running any command |
| Points or list items a `${...}` hands to a command | 512 | on substitution |
| Output of one command's parameters | 64 KiB of JSON | on substitution |

- A repeat count that is negative, fractional, `NaN`, infinite or over `max` refuses the code; the
  count is checked once, before a single command is built.
- **Repeats do not nest.** A `repeat` belongs to one step and that step holds no steps, so counts
  cannot multiply; the 512 expanded-command cap covers the sum anyway.
- Conditionals are expressions with no side effects; `when` only skips a step.
- Polygons come from table cells, which are bounded at load; an expression cannot grow a list (the
  only list makers are table cells and `flags`).
- No part of the path panics, reads a file other than the package, or reaches the network.

## 10. Answers to the first adopter's questions

1. **Grammar:** structured tokens plus `rules` with per-reason refusal texts, `not_modelled` refusals
   and defaults (section 2). The RFC 0019 regex is accepted as a declaration, checked by examples,
   and maps to tokens mechanically.
2. **Tables:** CSV with `source`, `page`, `status`, `note` columns and typed columns; lookups by
   one or more keys, chained (section 3). The UI shows them.
3. **Expressions:** arithmetic, `min`/`max`, trig, `fmt`/`concat`, conditionals, `when`, `repeat`,
   named variables (sections 4 and 5).
4. **Polygons:** a `points` column feeds `"${s.polygon}"` (section 5).
5. **Checks:** volume, bbox, faces, dimensions per code, plus valid/invalid examples (section 7).
6. **Provenance and guarantee:** sources, per-row status, text, and a guarantee level the index
   verifies (section 6).
7. **Version:** `format` plus `min_solidcraft`; one `.generator.json` file (section 1).
8. **CLI:** `generator.build` and `generator.check` through `solidcraft-cli run` (section 8).
