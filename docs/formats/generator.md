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
| Per-row source/page note | **Missing** in the table (`ConfigRow` has `id`, `name`, `cells` only). Provenance lives in the generator sidecar (section 4), so no design-format change is needed. | `crates/doc/src/config.rs` |
| `component.insert_design {path, configuration}` | Yes. The link stores the resolved row id. | `crates/engine/src/cmd/component_link.rs` |
| `insert_design` with FREE parameter values beyond a row | **Missing.** The link stores only the row. This work adds `parameters` (a map name -> expression) to `component.insert_design`, `component.relink` and the link, saved with the design through ONE pending format file; `component.update_links` and a later `component.set_link_parameters` (same map) rebuild the linked component. | `crates/engine/src/schema/component_link.rs`, `crates/doc/src/document.rs` (`DesignLink`) |
| Limits on a linked part | **Missing** for ordinary edits: range rules are checked at row activation/preview, and an ordinary `parameters.change` does not enforce them (`docs/configurations.md`, "Ordered rules"). This work checks the source's range rules and allowed values against the link's `parameters` when a link is inserted, relinked or updated, and refuses out-of-range values with the rule's name. | new, in `component_link.rs` |
| Suppress-if | Yes, through `drive_suppression` rules (the source's rules run while the link's row and overrides are applied). There is no inline `suppress_if` expression on a feature. | `config_rules.rs` |
| Measurement | `measure` (tool; command form in `crates/engine/src/cmd/measure_*.rs`): per body `volume_mm3`, `area`, `bbox {min,max}`, `faces`/edges/vertices, `closed`; `summary: true` keeps it short. `document.inspect` lists bodies with the same numbers. `check_parameters {set: {name: value}, report: [measure, errors]}` tries values and restores. | `docs/mcp.md` |
| Rhai, field templates, ordering-code commands | **Added by this work** (sections 3 to 7). A first draft of `generator.parse|build|check|list` over a JSON package exists on this branch and is rewritten onto the new design. | `crates/engine/src/generator/` |

## 2. Package layout

A generator is a **folder** (or a zip of it, extension `.scgen`) named by its id (lower-case
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
- A package totals <= 4 MiB, <= 64 files; unknown keys in `generator.toml` are refused except
  `x-` keys; `format = "solidcraft-generator/2"`; `min_solidcraft = "0.52.0"` (an older build
  says which version it needs).
- No file is read outside the folder. No network, no other program.

`generator.toml`:

```toml
format = "solidcraft-generator/2"
id = "mgn-rail"
name = "HIWIN MGN linear guide set"
license = "CC0-1.0"
model = "mgn-rail.sc"

[parameters]                     # which user parameters a code can set, with limits and units
size   = { type = "choice", values = ["7", "9", "12", "15"] }
length = { type = "int", min = 40, max = 2000, unit = "mm" }
blocks = { type = "int", min = 1, max = 4 }
block  = { type = "choice", values = ["C", "H"], labels = { C = "standard", H = "heavy" } }

[[template]]                     # one or more; the first that matches wins
pattern = "MGN{size}{block}[E]{blocks}R{length}"
escape  = "[A-Za-z0-9-]+"        # per-field escape pattern (section 3); optional

[[template]]
pattern = "MGN{size}{block}{blocks}R{length}"
fields  = { E = false }

[rows."C-15"]                    # provenance per configuration row (section 6)
source = "HIWIN MGN catalogue, p. 14, table MGN15"
page = 14
status = "verified"              # verified | needs_review | not_modelled
```

## 3. Field-template syntax

A template is a string of **literals**, **fields** `{name}` / `{name: spec}`, **optional parts**
`[...]` and separators.

| Element | Syntax | Meaning |
|---|---|---|
| Literal | `MGN` | fixed text |
| Choice field | `{size}` with `size` a `choice` in `[parameters]`, or `{size: 7\|9\|12\|15}` | the longest listed value that fits |
| Letter to value map | `{block: C=standard, H=heavy}` | the code letter stands for a value; printing writes the letter back |
| Typed number | `{length: int 40..2000}`, `{w: decimal 0.5..9 step 0.5 digits 1}` | range, optional step counted from min, no leading zeros (an int) |
| Optional part | `[E]`, `[-{coat}]` | may be left out. Optional literal `E` sets nothing; a name `[E:flag]` sets boolean `flag`. Defaults come from `[parameters]` (`default = ...`) |
| Free text | `{id: text 1..8}` | 1..8 characters matching the escape pattern |
| Separators and case | `-`, space, `_`, `/`, `.` | ignored when parsing; case ignored; printing uses the template's own spelling |

Several `[[template]]` entries are tried in order; the first complete match wins (ambiguity
between templates that both match is a check-time error in `generator.check`). One **escape
pattern** per field is allowed: a regular-expression-free character class language
(`[A-Za-z0-9-]`, `+`, `*`, `{n,m}` only) for characters not otherwise legal in a free-text field;
it is checked for ReDoS (no nesting, no alternation) and matched by hand-written code.

Round trip: `parse(print(p)) == p` for every parameter set a template can print, and
`print(parse(c))` returns the canonical spelling of `c`. Printing a parameter set a template
cannot express (a value outside its map or range) is a refusal naming the field.

Hostile-input limits: a code is at most 128 characters, a template at most 256, 32 fields,
matching is bounded to 20,000 steps; any breach is a typed refusal, never a panic.

Rust API (`crates/engine/src/generator/template.rs`):
`Template::parse(&str) -> Result<Template, TemplateError>`,
`Template::match_code(&self, code, &Fields) -> Result<ParamValues, Refusal>`,
`Template::print(&self, &ParamValues) -> Result<String, Refusal>`.
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
| `component.set_link_parameters` | `{component, occurrence?, parameters}` | `{updated}`; rebuilds within limits |

Measurement for a `generator.check` or a harness: `measure {body, summary: true}` or
`document.inspect {bodies: true, timeline: "summary"}` on the inserted component, and
`check_parameters {set}` to try lengths without keeping them.

## 8. Fixture and tests

The engine ships a tiny hand-made generator (metric T-nut: `.sc` with `length`, a 4-row
configuration table, a range rule) used to test: parse, print round trip, insert from a code,
then edit `length` and see it rebuild; out-of-range refusal; hostile input and runaway scripts.
