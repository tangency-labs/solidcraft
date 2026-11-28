# Generators: interface (version 2)

A generator turns a manufacturer's ordering code (`MGN15CH2R600`) into a **parametric part** and
back. It is a parametric `.sc` modelled once in SolidCraft, a configuration table of catalogue
rows, and a short ordering-code template. Inserted parts stay parametric. No third-party program
runs; logic is sandboxed Rhai. Part of #22331 (RFC 0017). This replaces the version 1 JSON
package (`generator-package.md`), which is removed when the new backend lands.

This page is the interface the parts session models against. Section 1 states what SolidCraft
has today and what this work adds; sections 2 to 8 define the new pieces.

## 1. What exists today, and what is missing

| Need | Today | File |
|---|---|---|
| Expressions in feature inputs | Yes: sketch dimensions, extrude and cut distances, hole inputs, pattern count and spacing all take expressions over user parameters (`parameters.add`, `parameters.change`, `set_parameter`). | `crates/doc/src/expr.rs`, `crates/engine/src/cmd/parameters.rs` |
| Pattern COUNT from a parameter | **Yes.** `solid.pattern.rectangular` stores `count1`/`count2` as expressions; `pattern_count` evaluates them with `eval_count` (unitless, must be a whole number 1..MAX_INSTANCES). `floor`, `ceil`, `round`, `min`, `max` exist, so `floor((length - 20 mm) / 25 mm) + 1` works. A fractional or unit-carrying result is refused, so always wrap in `floor/ceil/round`. Sketch-pattern count has not been verified; use feature patterns. | `crates/doc/src/eval.rs` (`pattern_count`), `crates/doc/src/expr.rs` (`eval_count`) |
| Configuration table | Yes: `config.configure {params, suppress, materials, appearances}` creates columns, `config.row {name, values, from, rename, index, delete}` rows, `config.column`, `config.activate`, `config.preview`, `config.table`. Cells are parameter expressions, feature suppression, material, appearance. | `crates/engine/src/cmd/config.rs`, `docs/configurations.md` |
| Rules (limits, drives, suppression) | Yes: `config.rule {rule: {id, name, condition, effect}}`. Conditions: `always`, `parameter` (`eq ne lt le gt ge`), compound `all/any/not`. Effects: `drive_parameter`, `drive_suppression`, `parameter_range {parameter, minimum, maximum}` (inclusive), allowed values. A range rule with `condition: always` expresses "length within 40..2000 mm". "Suppress when length > 500 mm" is a `drive_suppression` rule with a `parameter` condition. | `crates/doc/src/config_rules.rs` |
| Per-row source/page note | Not in the table (`ConfigRow` has `id`, `name`, `cells` only). Provenance lives in the generator: `[rows.*]` in `generator.toml`, or the `source`, `page`, `status`, `note` and `description` columns of a `tables/*.csv` row whose first column is the configuration row's name (section 6). No design-format change is needed. | `crates/doc/src/config.rs` |
| `component.insert_design {path, configuration}` | Yes. The link stores the resolved row id. | `crates/engine/src/cmd/component_link.rs` |
| `insert_design` with FREE parameter values beyond a row | **Added by this work** (before it, `parameters` was accepted and ignored). The link stored only the row; it now has `parameters` (a map name -> expression) to `component.insert_design`, `component.relink` and the link, saved with the design through ONE pending format file; `component.update_links` and a later `component.set_link_parameters` (same map) rebuild the linked component. | `crates/engine/src/schema/component_link.rs`, `crates/doc/src/document.rs` (`DesignLink`) |
| Limits on a linked part | Range rules are checked at row activation/preview and an ordinary `parameters.change` in the source design still does not enforce them (`docs/configurations.md`, "Ordered rules"); enforcing them there would change a default and needs a preference, so it is left as a decision for the maintainers. This work checks the source's range rules and allowed values against the link's `parameters` when a link is inserted, relinked or updated, and refuses out-of-range values with the rule's name. | new, in `component_link.rs` |
| Suppress-if | Yes, through `drive_suppression` rules (the source's rules run while the link's row and overrides are applied). There is no inline `suppress_if` expression on a feature. | `config_rules.rs` |
| Measurement | `measure` (tool; command form in `crates/engine/src/cmd/measure_*.rs`): per body `volume_mm3`, `area`, `bbox {min,max}`, `faces`/edges/vertices, `closed`; `summary: true` keeps it short. `document.inspect` lists bodies with the same numbers. `check_parameters {set: {name: value}, report: [measure, errors]}` tries values and restores. | `docs/mcp.md` |
| Rhai, field templates, ordering-code commands | **Added by this work** (sections 3 to 7). A first draft of `generator.parse|build|check|list` over a JSON package exists on this branch and is rewritten onto the new design. | `crates/engine/src/generator/` |

## 2. Package layout

A generator is a **folder** named by its id (lower-case
letters, digits, dashes, 3..64 chars):

```
mgn-rail/
  generator.toml      required: metadata, templates, provenance
  mgn-rail.sc         required: the parametric model + its configuration table
  generator.rhai      optional: logic (section 5)
  tables/*.csv        optional: lookup tables read by `table()` (UTF-8, <= 256 KiB, <= 5000 rows)
```

- The `.sc` is an ordinary design. Its **user parameters** are the generator's parameters. Its
  **configuration table** holds one row per catalogue row (parameter columns, suppression
  columns). Its **rules** hold the declared limits (`parameter_range`) and conditional
  suppression. Nothing generator-specific is stored in the `.sc`, so the format does not change
  for the model itself.
- Limits: `generator.toml` <= 256 KiB, the script <= 32 KiB, each table <= 256 KiB, <= 32 tables,
  <= 32 templates, <= 512 examples; unknown keys in `generator.toml` are refused except `x-` keys; `format = "solidcraft-generator/2"`; `min_solidcraft = "0.52.0"` (an older build
  says which version it needs).
- No file is read outside the folder (a path with `..`, an absolute path or a link out of the
  folder is refused). No network, no other program. A zipped form (`.scgen`) is not read yet; use the folder.

`generator.toml`:

```toml
format = "solidcraft-generator/2"
id = "mgn-rail"
name = "HIWIN MGN linear guide set"
license = "CC0-1.0"
model = "mgn-rail.sc"
row_from = ["block", "size"]         # no script: the row is named by these fields joined with "-"

[parameters]                         # what a template's bare {name} means; units for numbers sent to the design
size   = { type = "choice", values = ["7", "9", "12", "15"] }
length = { type = "int", min = 40, max = 2000, step = 1, unit = "mm" }   # also: type = "decimal" (min, max, step, digits)
blocks = { type = "int", min = 1, max = 4, default = 1 }
block  = { type = "map", codes = { C = "standard", H = "heavy" } }         # letters to values
id     = { type = "text", min = 1, max = 8, escape = "[A-Za-z0-9/]" }      # free text and its extra characters

[[template]]                         # one or more; the first that matches wins
pattern = "MGN{size}{block}[E:long]{blocks}R{length}"
description = "MGN{size} rail, {length} mm"     # {field} filled in; becomes the component's description
fields = { shape = "rail" }                      # values this template adds when it matches (optional)

[rows."C-15"]                        # provenance for configuration row "C-15"
source = "HIWIN MGN catalogue, table MGN15"
page = 14
status = "verified"                  # verified | needs_review | not_modelled
note = ""
description = "MGN15 standard block"            # per-row description; beats the template's

[[example]]                          # checked by generator.check
code = "MGN15C1R600"
volume_mm3 = 12345.0
bbox_mm = [600.0, 15.0, 16.0]
```

`[parameters]` `type`s: `choice` (`values`), `map` (`codes`), `int` and `decimal` (`min`, `max`,
`step`, `digits`, `unit`), `text` (`min`, `max`, `escape`). `default` is the value an omitted
optional part gives. `unit` is written after a number sent to the design (`600 mm`). `labels`
may give display text for choice values.

## 3. Field-template syntax

A template is a string of **literals**, **fields** `{name}` / `{name: spec}`, **optional parts**
`[...]` and separators.

| Element | Syntax | Meaning |
|---|---|---|
| Literal | `MGN` | fixed text |
| Choice field | `{size}` with `size` a `choice` in `[parameters]`, or `{size: 7\|9\|12\|15}` | the longest listed value that fits |
| Letter to value map | `{block: C=standard, H=heavy}` | the code letter stands for a value; printing writes the letter back |
| Typed number | `{length: int 40..2000}`, `{w: decimal 0.5..9 step 0.5 digits 1}` | range, optional step counted from min, no leading zeros (an int) |
| Optional part | `[E]`, `[-{coat}]` | may be left out. `[E]` alone sets nothing; `[E:flag]` sets boolean `flag` (true when present, false when not). Fields inside get their `[parameters]` `default` when the part is left out, and the part is printed only when a field differs from its default |
| Free text | `{id: text 1..8}` | 1..8 letters and digits; extra characters come from the declared parameter's `escape` |
| Option set | `{opts: set(-LTP, -RTP, -SC:int 0..99)}` | options in any order, each at most once, some with an argument. The value is an object (`{"LTP": true, "SC": 12}`); printing uses the declared order |
| Free tail | `[+{tail: rest}]` | everything left of the code as typed (1..64 characters); put it last |
| Required separator | `E-SFJ{d: int 1..99}\-{l: int 1..99}` | `\-` must be present as written, so `E-SFJ12-5` reads and `E-SFJ125` does not |
| Separators and case | `-`, space, `_`, `/`, `.` | ignored when parsing (except a required one); case ignored; printing uses the template's own spelling |

Several `[[template]]` entries are tried in order; the first complete match wins (ambiguity
between templates that both match is a check-time error in `generator.check`). A free-text
parameter may declare one **escape pattern** (`escape = "[A-Za-z0-9/]"`): a character-class
list (ranges and single characters) for the extra characters the field accepts; there is no
regular-expression engine, so it cannot run away.

Round trip: `parse(print(p)) == p` for every parameter set a template can print, and
`print(parse(c))` returns the canonical spelling of `c`. Printing a parameter set a template
cannot express (a value outside its map or range) is a refusal naming the field.

Hostile-input limits: a code is at most 128 characters, a template at most 256, 32 fields,
matching is bounded to 20,000 steps; any breach is a typed refusal, never a panic.

Rust API (`crates/engine/src/generator/code_template.rs`):
`Template::parse(&str, &Declared) -> Result<Template, TemplateError>`,
`Template::match_code(&self, &str) -> Result<Option<Params>, TemplateError>`,
`Template::print(&self, &Params) -> Result<String, TemplateError>`, `Template::prefixes()` (the
literal prefix of every code, for an index) and `match_any(&[Template], &str)`.
Command forms: `generator.parse` and `generator.code` below. A tool-callable template-only form
is `generator.template {template, code?, parameters?}` -> `{parameters}` or `{code}`, for
trying a template before a package exists.

## 4. From code to a part: the mapping

`generator.parse` returns `{parameters, configuration}`:

1. The matched template gives **fields** (typed values: `size = "15"`, `block = "C"`, `length = 600`).
2. The Rhai `resolve(fields)` function (section 5) turns fields into
   `{configuration: "C-15" | null, parameters: {length: "600 mm", ...}}`. With no script, the
   default rule is: `configuration` = the row whose name equals the fields joined by `-`
   (`block-size`), else the row named by `[parameters]`' `row_from = ["block", "size"]`; every
   numeric field named like a user parameter becomes an override.
3. The row and every override are checked against the source's declared limits (`parameter_range`
   rules and `[parameters]` ranges); a value outside refuses with the field and limit.
4. `generator.insert` inserts the `.sc` with `configuration` and `parameters`. The link keeps
   both, so editing `length` later (`component.set_link_parameters`) rebuilds the part within
   its limits, and `component.update_links` re-reads the file.

## 5. Rhai

`generator.rhai` may define any of these (all optional, plain Rhai):

```rhai
fn resolve(fields)  // -> #{ configuration: "C-15", parameters: #{ length: "600 mm" } } or refuse(...)
fn describe(parameters) // -> text shown beside the code (optional)
```

Registered functions:

| Function | Meaning |
|---|---|
| `code_template(fields_or_params)` | the printed code for the current template (same as `generator.code`) |
| `table(name, key)` / `table(name, key, column)` | one row/cell of `tables/<name>.csv` by first-column key; unknown key returns `()` |
| `refuse(kind, message)` | stop with a refusal: `kind` is `invalid` (not a valid code) or `not_modelled` (valid, not drawn yet); the message is shown verbatim |

Sandbox: Rhai with the standard package only; **no** file, network, time, process, `import`,
`eval`; limits `max_operations = 100_000`, `max_call_levels = 16`, `max_expr_depth = 32`
(functions 16), `max_string_size = 4096`, `max_array_size = 1024`, `max_map_size = 256`; a
script of at most 32 KiB. Every breach is a typed error (`script_limit`, `script_error`),
never a hang or a panic. Builds for wasm. A script cannot change the design: it only returns data.

## 6. Provenance and review

Each configuration row may have a `[rows."<name>"]` entry: `source` (text), `page`, `status`
(`verified`, `needs_review`, `not_modelled`), and optional `note`. `generator.parse` returns the
row's provenance; the Insert dialog and the component's properties show it; a `needs_review`
row inserts with a warning, a `not_modelled` row refuses. CSV tables carry `source`, `page`,
`status` columns that `table()` returns and the same rules apply. The component built from a
code records `generator`, `code`, `row` and the provenance in its properties (the part number is
the printed code).

## 7. Commands

All are normal commands with input and output schemas (optional fields use
`skip_serializing_if`), callable over MCP `execute`, scripts and the UI.

| Command | Input | Output |
|---|---|---|
| `generator.list` | `{}` | `{generators: [{id, name, templates, license, source: "installed" \| "path"}]}` |
| `generator.parse` | `{generator, code}` (`generator` = installed id or folder path) | `{ok: true, parameters: {name: expr}, configuration: row-name \| null, template, fields, provenance}` or `{ok: false, reason: "invalid" \| "not_modelled" \| "limit", message}`. A refused code is an answer, not an error. Never changes the design |
| `generator.code` | `{generator, parameters, template?}` | `{code, template}` or a refusal naming the field |
| `generator.insert` | `{generator, code}` or `{generator, parameters, configuration?}`, plus `placement?` (the `component.insert_design` placement keys) | `{component, occurrence, code, configuration, parameters, provenance}`. One undo step. A refusal is an error and leaves the design unchanged |
| `generator.template` | `{template, code?, parameters?}` | `{parameters}` or `{code}` |
| `generator.check` | `{generator}` | `{passed, results: [{code, ok, problems}]}` from the package's declared `[[example]]` codes: each parses, round-trips, inserts into a scratch design and its measurements match `[[example]] volume_mm3 / bbox_mm / faces` |
| `component.set_link_parameters` | `{component, parameters}` (an object; `null` clears) | `{component, parameters, updated}`; replaces the link's parameters and rebuilds within limits; one undo step. All placements of the component follow |

Measurement for a `generator.check` or a harness: `measure {body, summary: true}` or
`document.inspect {bodies: true, timeline: "summary"}` on the inserted component, and
`check_parameters {set}` to try lengths without keeping them.

## 8. Fixture and tests

The engine ships a tiny hand-made generator (metric T-nut: `.sc` with `length`, a 4-row
configuration table, a range rule) used to test: parse, print round trip, insert from a code,
then edit `length` and see it rebuild; out-of-range refusal; hostile input and runaway scripts.

## 9. Details the parts session asked about

- **`fields` holds the mapped value, and the letter next to it.** For `{block: C=standard, H=heavy}`
  a code with `C` gives `fields.block = "standard"` and `fields.block_code = "C"`. Printing reads the
  value (`block`); `block_code` is for scripts and display only.
- **Flags and absent parts.** `[E:long]` sets `fields.long = true` when the part is typed and `false` when
  it is not. A field inside an optional part that is left out gets its `[parameters]` `default`; with no
  default the key is absent (a script reads it as `()`). An option set always gives an object (`{}` when
  none was typed).
- **Options outside the code** (the caps of a rail set, say): `generator.parse {generator, code, options}`
  and `generator.insert {generator, code, options}` take an `options` object. It reaches the script as
  `fields.options` (a key named `options` that a template itself sets wins). The printed code does not
  contain them.
- **`table()`** takes a text or an integer key: `table("rails", 15)` and `table("rails", "15")` are the same
  row, and the key is matched ignoring case.
- **Provenance from the CSV.** If a configuration row named `K` has no `[rows.K]` in `generator.toml`, the
  first table with a row keyed `K` that has `source` or `status` columns supplies it (`source`, `page`,
  `status`, `note`, `description`). A `[rows.K]` entry wins.
- **Limits on a linked part.** `component.insert_design {parameters}`, `component.relink {parameters}` and
  `component.set_link_parameters` refuse a value outside the source's `parameter_range` or allowed-values
  rules, naming the rule, and refuse a name the source does not have. A range, allowed-values or
  suppression rule no longer needs its parameter to be a column of the configuration table. Rules may
  name a feature by its name (`"feature": "Rail cap 1"`).
- **Changing one value later.** `component.set_link_parameters {component, parameters, merge: true}` changes
  only the given parameters. `parameters.change` in the assembly does not reach into a linked part.
- **Not done in this change** (follow-ups): `body_names` for pattern copies, a thread designation driven by a
  parameter, and `model.threads` on every copy of a patterned thread.
