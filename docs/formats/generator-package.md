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
  must not pass silently), except `x-` keys, which are allowed on the top level and inside every structural object (grammar and its
  tokens, tables, lookups, rules, variables, steps and blocks, repeats, sources, `review`, checks and `insert`) and are
  ignored. Command parameters (`params`) and `declares` are the package's own data and are not touched.
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
| `{"field": "w", "decimal": {"min": 0.5, "max": 9, "step": 0.5, "digits": 1}}` | a decimal with at most `digits` places; a `step` is counted from `min` (`min`, `min + step`, ...; 0 or no step: any value) |
| `{"field": "id", "chars": "digits"\|"alpha"\|"alnum", "min_len": 1, "max_len": 8}` | a run of at least `min_len` and at most `max_len` characters of the class (the longest that lets the rest match), kept as typed |
| `{"optional": [tokens], "default": {"m": "5"}}` | tokens that may be left out; `default` gives the field values when they are |
| `{"flags": {"field": "options", "options": [{"code": "-C", "value": "coated"}], "max": 3}}` | optional suffixes, each at most once, in any order; options are tried longest code first and `max` counts the options given; the field is the list of values given, in the order typed (empty when none) |

**Case.** `nocase` on a `lit` or `enum` matches either case and gives the listed spelling. For a code typed in
any case, `"grammar": {"normalize": "upper"}` (or `"lower"`) folds the whole code (ASCII letters) before it is
matched; the folded code is `code` in expressions, the component's part number and the name, while `{code}` in a
`grammar.refuse` or `missing` text is the code as it was typed (trimmed, the first 40 characters).

Every field has a value after parsing, either typed from the code or from a `default`. A field is
text, a number, or (for `flags`) a list of text. The parse result is the **fields** object, shown in
the dialog.

**Cross-field constraints, refusals and defaults** are `rules`, run after parsing and after the
lookups of section 3, in order. A rule is

```json
{"when": "type == \"6\" && m != \"M5\"", "refuse": "HNTT{type} is offered in M3, M4, M5, not {m}", "kind": "invalid"}
```

`when` is a boolean expression (section 4). When it is true the code is refused with `refuse`
(`{name}` in the text is replaced by a field, lookup value or variable). `phase` is `late` (the
default: after the lookups and variables) or `early` (before the lookups, so a rule can refuse a valid-but-undrawn
type that has no table row at all; it reads fields, defaults and constants only). A lookup that finds no row
refuses with its `missing` text and its `missing_kind` (`invalid` by default, or `not_modelled`). `kind` is `invalid` (the
default: the code is not an ordering code of this series) or `not_modelled` (a real code this
generator does not model; the dialog says so in different words and the command reply says
`"reason": "not_modelled"`). Defaults are in the grammar (`default` above) and in `defaults`, a
`{field: expression}` object for values that depend on others (used only when the field is
absent). See "Order of evaluation" below.

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

- `csv` is comma-separated with a header row; a cell may be quoted with `"` (`""` is a quote); at most
  5000 rows and 64 columns. Column types and what an empty cell means:

  | Type | Cell | Empty cell |
  |---|---|---|
  | `text` | any text | empty text |
  | `number` | a finite number | an error: the table is refused (use `number?`) |
  | `number?` | a finite number, or nothing | no value: `has(x)` is false, `${x}` is `null`, arithmetic on it is an error |
  | `list` | space-separated texts | an empty list |
  | `numbers` | space-separated finite numbers (`1 2 3`) | an empty list |
  | `points` | pairs `x y` separated by `;` (`0 0; 10 0`), finite | an empty list |
  | `tuples` | groups of 1 to 16 numbers separated by `;` (`1 2 3; 4 5 6`), a list of lists | an empty list |

  At most 512 items per cell. A cell that does not fit its type refuses the package at load,
  naming the row and column. A list, numbers, points or tuples cell is read with `len(x)` and
  `at(x, i)` (`at` of a `points` or `tuples` cell is the pair or group, a list), so a profile with a
  different number of bores per row is one `repeat` over `len(...)` (section 5).
- `key` names the columns that identify a row; a repeated key refuses the package.
- Reserved provenance columns, all optional: `source` (an id from `sources`), `page`, `status` and
  `note`. They are not dimensions, but the UI shows them as the part's provenance (section 6).
  A `status` is `published` (a number the source prints), `labelled` (a number on the drawing's
  label), `measured` (taken from the source's drawing), `derived` (worked out from other numbers) or
  `approximate` (a guess; the UI flags it). A row whose columns differ lists them:
  `measured:d0,floor_half;approximate:tip_round;published` means `d0` and `floor_half` are measured,
  `tip_round` is approximate and every other column is published; a bare status is the default for
  the columns not listed, and a named column that the table does not have refuses the table.
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

- `const` (top level) names values that every expression can read, and a lookup can use as a key:
  `"const": {"plan": "plan", "ratio": 0.5}` and `"key": {"name": "plan"}`. A constant is a finite number,
  text or true/false; its name may not be reused by a field, default, lookup or variable, and `code`
  is reserved for the code itself.

## 4. Expressions

A tiny pure language, evaluated by SolidCraft's own evaluator (no I/O, no loops, no calls out, no
recursion). Values are numbers (finite, `f64`), text, booleans and lists.

- Numbers: `12`, `0.5`, `1e-3`. Text: `"M4"`. References: a field, lookup record column
  (`s.E`), variable, or a `repeat` index.
- Operators: `+ - * / %` (`+` also joins text), unary `-` and `!`, `== != < <= > >=`, `&& ||`
  (short circuit), `c ? a : b`, parentheses.
- Functions: `min max abs round floor ceil sqrt pow`, `sin cos tan asin acos atan atan2` (degrees),
  `pi()`, `if(c, a, b)`, `in(x, list)`, `len(list)`, `at(list, i)`, `join(list, sep)`,
  `fmt(x)` and `fmt(x, digits)` (number to text, below), `has(x)` (an empty `number?` cell is false), `concat(a, b, ...)`, `upper lower`.
- **Number to text** is defined once: wherever a number becomes text (`${...}` inside longer text, `{name}` in a
  message, `+` with text, `concat`, `fmt(x)`) a whole number is written without a decimal point (`497`, `1000`,
  `-0` is `0`) and any other number as the shortest text that reads back as the same number (`4.2`, `497.5`, never an
  exponent). `fmt(x, d)` writes exactly `d` decimals (0 to 12).
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
  1024), else the code is refused. A build expands at most 512 commands in all.
- **A block** repeats or guards several commands together, in order. `{"repeat": {...}, "steps": [A, B]}` runs
  A then B for `i = 0`, then A then B for `i = 1`, and so on (a polyline run followed by its arc, again and again),
  not all the A and then all the B. A block holds 1 to 64 commands, each `{command, params?, when?}` (its own `when`
  is tested for every pass); it has no `params` of its own, and it holds no repeat and no block, so repeats still
  cannot nest. A step is a `command` or a block of `steps`, never both. The count `times x commands` is added to
  the running total and checked against the 512-command cap before anything is built, and the template as a
  whole holds at most 256 commands (each counted once).
- Only commands in the **allowed list** may be used: the commands whose ids start with `sketch.`, `solid.`,
  `component.`, `construct.`, `surface.`, `parameters.` and `sheet.`, and exactly `browser.visibility`
  (hide the construction sketch), `document.metadata.set` and `component.metadata.set` (part number,
  description, material). Anything else (file, plugin, network, settings, shell, generator, and any command
  with `import`, `link`, `open`, `export`, `save`, `load`, `insert_`, `paste` or `from_file` in its id) is refused
  when the package loads, as is any parameter named `file`, `filename`, `url`, `uri`, `directory` or `folder`.
- Each step runs through `Session::execute`, so the command's own schema check, undo and errors
  apply. The whole build is one undo step: if any step fails, the design is left as it was, and the
  error names the failing step and command.
- `insert` says where the part goes, and it is not a step: the runner, not the package, issues the commands for it.
  `{"component": "${code}"}` (the default) runs `component.create` with that name (activated), then
  `component.metadata.set`, then the package's commands, then `component.activate.parent`, so the part is a
  component of its own and the person is where they were. `{"component": false}` builds the commands in the
  active component (or the document) as they are, for a package that places everything itself. The component's part number is the code
  and its description is the package's `name`, so a bill of materials names the part as ordered.

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
- `review` is the evidence behind the guarantee, shown with it and never trusted: `{"reviewer": "A. Reader",
  "reviewer_kind": "person" | "ai" | "automatic", "date": "2026-11-27", "sources_hash": "sha256:...",
  "level": "reviewed", "notes": "..."}`. The Workshop index build checks it and sets the badge.
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

How a check measures, so a maker's build and SolidCraft's agree: `volume_mm3` is the total volume of the bodies
the build made, `bbox_mm` the size (max minus min, x y z) of their joint bounding box, `faces` the sum of their
merged face counts as `inspect.measure` reports them (`faces` in its reply: split pieces of one analytic surface
count once; it is not the kernel's own face count nor the length of `model.faces`), and `dimensions` the value of
a variable or field by name. When a check gives no tolerance: volume `rel_tol` 1e-6 (relative), bounding box
`abs_tol` 1e-6 mm per side, `dimensions` exact to 1e-9, and `faces` exact.

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

### Order of evaluation

For one code, in this order, each step able to refuse the code:

1. `const` values are in scope. 2. The grammar (after `normalize`) gives the fields. 3. `defaults` give the
fields that are still missing, each worked out from the fields and constants alone (a default does not see another
default). 4. `rules` with `"phase": "early"`. 5. `lookups`, in order (a later one may use an earlier one's
columns). 6. `vars`, in order (a variable sees only earlier ones). 7. `rules` that are `late` (the default).
8. `steps`: each step's `when`, then its `repeat` count (checked before anything is built), then each command's
`when` and `params` for every pass. The package never runs anything in another order.

## 9. Installing and using a package

SolidCraft reads packages from the `generators` folder of its per-user data folder
(`~/.local/share/solidcraft/generators` on Linux, `%LOCALAPPDATA%\SolidCraft\generators` on Windows,
`~/Library/Application Support/SolidCraft/generators` on macOS), or from the folder the
`SOLIDCRAFT_GENERATORS_DIR` environment variable names. A file there named `<id>.generator.json`
is installed. **Insert > Library > Ordering Code** opens a dialog: choose a generator, type a code,
read what SolidCraft made of it (the fields, the table rows used with their sources and status,
the package's provenance text, or the package's own reason for refusing the code) and insert the
part. Nothing is downloaded or run by the dialog; the Workshop's own ask-first install is what
puts a package in the folder.

## 10. Limits and hostile input (the format stays data)

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
| Table | 5000 rows, 64 columns, 512 items in a list cell, 16 numbers in a tuple | at load |
| Points in one cell | 512 | at load |
| Expression | 512 characters, 256 parts, nesting 32 | at load |
| Whole build | 1,000,000 evaluation steps | at evaluation |
| Commands in the template (blocks counted by their commands) | 256; a block holds at most 64 | at load |
| Repeat count | a whole number, 0 to its `max`; `max` at most 1024 | after evaluating the count and **before expanding** |
| Expanded commands (all repeats together) | 512 | before running any command |
| Points or list items a `${...}` hands to a command | 512 | on substitution |
| Output of one command's parameters | 64 KiB of JSON | on substitution |

- A repeat count that is negative, fractional, `NaN`, infinite or over `max` refuses the code; the
  count is checked once, before a single command is built.
- **Repeats do not nest.** A `repeat` belongs to one step (or one block of plain commands), and a block holds no
  repeat and no block, so counts cannot multiply; a block's commands times its count is added to the 512-command
  cap before anything is expanded.
- Conditionals are expressions with no side effects; `when` only skips a step or a command.
- Polygons come from table cells, which are bounded at load; an expression cannot grow a list (the
  only list makers are table cells and `flags`).
- No part of the path panics, reads a file other than the package, or reaches the network.

## 11. Version, and answers to the first adopter's questions

**Version.** The format tag is `solidcraft-generator/1`. The additions in this section are all optional
keys and column types (`const`, `review`, `phase`, `missing_kind`, `normalize`, block `steps`, `insert.component:
false`, `number?`, `numbers`, `tuples`, a status per column, `fmt(x)`, `has(x)`, `x-` keys in inner objects), so a
package written before them loads unchanged and the tag stays `/1`; a package that uses one sets `min_solidcraft` to
the first release that has it. A change that would make an older package mean something else gets `/2`.

| # | Question | Answer |
|---|---|---|
| 1 | A refusal for a valid-but-undrawn type before the lookup | `rules[].phase: "early"`, and `lookups[].missing_kind: "not_modelled"` (section 2) |
| 2 | Case folding | `nocase` on `lit`/`enum`, and `grammar.normalize: "upper" \| "lower"` (section 2) |
| 3 | Number to text | one rule, `fmt(x)` and `fmt(x, d)` (section 4) |
| 4 | Empty cells | legal for list, numbers, points and tuples (an empty list); `number?` for "none", `has(x)` (section 3) |
| 5 | Numeric lists | `numbers` and `tuples` columns, read with `len` and `at`, one `repeat` over `len(...)` (sections 3 and 5) |
| 6 | Repeat a group of steps | a block: `{"repeat": ..., "steps": [...]}`, interleaved in order (section 5) |
| 7 | Order of evaluation, constants | stated (after section 8); `const` (section 3) |
| 8 | Allow-list, `insert` | `browser.visibility`, `component.metadata.set` and `document.metadata.set` are allowed; `insert` runs `component.create` (activated), `component.metadata.set`, the commands, `component.activate.parent`; `insert.component: false` builds in place (section 5) |
| 9 | Status per column | a `status` cell may list columns: `measured:d0,floor_half;approximate:tip_round;published` (section 3), plus the `labelled` status |
| 10 | Guarantee evidence | the `review` object (section 6) |
| 11 | How `faces` is counted, tolerances | merged face count of `inspect.measure`, volume 1e-6 relative, box 1e-6 mm (section 7) |
| 12 | Small things | decimal `step` from `min`, `chars`, `flags`, `{code}` as typed, `x-` keys inside every structural object (section 2, 1) |

Earlier questions: grammar with per-reason refusals, defaults and `not_modelled` (section 2); tables with
provenance columns and chained lookups (section 3); arithmetic, trig, `fmt`, conditionals, `repeat`, named variables
(sections 4 and 5); polygons from a `points` column (section 5); checks per code (section 7); sources, per-row status,
guarantee (section 6); one `.generator.json` file with a format tag and `min_solidcraft` (section 1); the CLI path
`generator.build` and `generator.check` through `solidcraft-cli run` (section 8).
