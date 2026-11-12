# RFC 0005: Intent

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Position | An optional notebook; skip it for a quick build |
| Maps to | phase P0 of [RFC 0004](0004-product-workflow.md) |
| Works with | an optional product folder, or a single `.sc` design file |
| Detailed plan | this RFC; implementation slices below |

## Summary

Intent is the cheapest useful tool in the Inventor's Toolbox: a place to write down the problem, the
people it is for, the requirements, the limits, the risks and the decisions — when the user wants to.
It is a **notebook**, not a mandatory first step. "New Design" works exactly as it does today, and a
person can model, simulate, prototype and release without ever opening it.

The tool is a structured, validated, four-surface **Product Brief** with a guided GUI, templates, a
help page and examples. It is useful for anything that will be remembered, shared, reviewed or
returned to. It never locks a tool, never blocks a command and never turns a missing field into debt.

## 1. The problem

- Some people want a place to record intent and limits, and there is none.
- Requirements live in someone's head, a notebook or a chat, and are lost when the project is
  picked up again.
- Cost, mass, power and size limits are discovered late, even when the user wanted to write them
  down early.
- There is no record of why a choice was made, so it is re-litigated or silently reversed.
- Agents can model a part but cannot read the user's intent when it was written down somewhere else.
- The answer is a notebook that is available, not a form that is required.

## 2. Goals and non-goals

**Goals**

- A brief that a beginner can complete with guidance and an expert can complete in minutes.
- Typed, validated fields where validation helps; free text where it belongs.
- The tool is discoverable but never in the way: one click to add, one click to ignore.
- Every requirement can have an acceptance test; every budget can have a unit and a target; every
  risk can have a mitigation and an owner — when the user chooses to record them.
- Budgets can be compared with design and simulation measurements in one view.
- The brief is readable and writable from the GUI, CLI, MCP and documentation.
- Templates for common products, and a help page that explains each field with examples.
- A readiness report that suggests what is missing, without blocking anything.

**Non-goals**

- A required phase, a gate, a waiver system or a forced wizard.
- A requirements-management suite, PLM or a general document editor.
- Market research, pricing strategy or business planning.
- Certification or legal advice.

## 3. What the brief can hold

All sections are optional. The editor shows a progress count for the user's own benefit, not a
completion requirement.

| Section | Content | Validation when used |
|---|---|---|
| I1 Problem and users | One-line pitch, problem, users, jobs, non-goals | Non-empty problem if the section is started |
| I2 Requirements | Must/should/could, statement, acceptance test, owner, links | A must requirement has an acceptance test |
| I3 Success criteria | Metric, target, unit, measurement method, linked requirement | Every criterion has a unit |
| I4 Budgets | Mass, power, cost, size, time, environment; target, tolerance, unit | Every budget has a unit and a target |
| I5 Risks | Description, likelihood, impact, mitigation, owner, trigger, cheap check | Readiness hint: a high risk should have a mitigation |
| I6 Compliance screen | Mains, battery, RF, laser, heat, medical, toy, vehicle, outdoor, waste | Readiness hint: a triggered topic should have a note |
| I7 Decisions | Date, options, choice, reason, revisit condition | A decision has a reason |
| I8 Feasibility plan | Unknown, method, prototype, exit criterion, order | An item has a next step |
| I9 Help and templates | Guided fields, examples, product templates | Every field has help text |

## 4. Data model

The brief is structured data. Markdown/PDF are exports, not the source. It can live in an optional
product folder or in the design; a design without one is complete.

```text
ProductBrief {
  schema: 1,
  name, one_liner,
  problem,
  users: [{ name, description, jobs[] }],
  non_goals: [string],
  requirements: [{
    id, statement, priority: must|should|could,
    acceptance, owner, links: [{ kind, target }]
  }],
  success: [{ requirement, metric, target, unit, method }],
  budgets: [{
    kind: mass|power|cost|size|time|temperature|other,
    target, tolerance, unit, notes
  }],
  risks: [{
    id, description, likelihood: low|medium|high,
    impact: low|medium|high, mitigation, owner, trigger, check
  }],
  compliance: [{
    topic, triggered, decision: in_scope|out_of_scope|test_house,
    notes
  }],
  decisions: [{ id, date, options[], choice, reason, revisit }],
  feasibility: [{ unknown, method, prototype, exit, order }],
  template: { id, revision }
}
```

Ids are stable. Deleting a requirement keeps its links visible as broken until re-pointed or
removed, the same rule the design uses for face references.

## 5. Commands

| Command | Does |
|---|---|
| `brief.create` | Create an empty brief or one from a template |
| `brief.get` | Read the whole brief, validated |
| `brief.set` | Replace the brief atomically; refuses invalid data |
| `brief.requirement.add` / `.edit` / `.remove` | Requirement rows |
| `brief.success.set` | Success criteria for a requirement |
| `brief.budget.set` | A budget target, tolerance and unit |
| `brief.risk.add` / `.edit` / `.remove` | Risk rows |
| `brief.compliance.set` | A screening answer and note |
| `brief.decision.add` / `.edit` | Decision log |
| `brief.feasibility.set` | Feasibility plan rows |
| `brief.readiness` | Advisory check and suggested next tools; never blocks |
| `brief.export` | Markdown, PDF or JSON to a path |
| `workflow.status` / `workflow.readiness` | Shared map and advisory checks from RFC 0004 |

Every command has an input and output schema, typed errors, and never opens a dialog. A failed edit
leaves the brief unchanged. Creating or not creating a brief is not a state that any other command
depends on.

## 6. GUI

- **Start a design** remains the first action. **Add a brief** is offered beside it, not before it.
- **INTENT** is an optional tab, hidden until a brief exists or the user opens it from the palette.
- **Guided fields**: each field has a one-line explanation, an example and a "why this matters" link
  to the help page.
- **Template gallery**: enclosure + board + firmware, mechanical-only, electronics-only, blank.
- **Budget view**: target vs measured, with the source of each measurement (mass properties, BOM
  cost, simulation, test). It appears when both a target and a measurement exist.
- **Risk matrix**: likelihood × impact, with the cheap check and owner.
- **Compliance notes**: yes/no triggers and a note per trigger; no certificate, no gate.
- **Decision log**: a table with reasons and revisit conditions.
- **Readiness**: a small advisory list, dismissible, with links to the tools that fill the gaps.
- **Export** to Markdown/PDF for a review or a collaborator.

The tab is useful empty, and the app is useful without it.

## 7. CLI

```bash
# Skip the brief entirely: nothing is required.
solidcraft-cli exec solid.box '{"length":40,"width":20,"height":5}'

# Add one later, when it is useful.
solidcraft-cli exec brief.create '{"product":".","template":"enclosure-board-firmware"}'
solidcraft-cli exec brief.requirement.add '{"id":"R1","statement":"Hold 2 kg at 40 °C","priority":"must","acceptance":"static test at 2 kg"}'
solidcraft-cli exec brief.budget.set '{"kind":"mass","target":250,"tolerance":25,"unit":"g"}'
solidcraft-cli exec brief.readiness '{}'
solidcraft-cli exec brief.export '{"path":"docs/brief.md","format":"md"}'
```

Headless CI may run `brief.readiness` for information. It must not fail a build because a brief is
missing; only a user who asked for the check can act on it.

## 8. MCP server

- Every command above is a tool with its parameter docs and output schema.
- The brief is also exposed as a **resource** (Markdown and JSON) when it exists, so an agent can
  read intent before modelling and cite the requirement it is satisfying.
- When no brief exists, the tool says so and continues. It never refuses to model.
- A write is explicit; an agent cannot silently change a requirement or budget.
- The readiness report is a tool result, so an agent can suggest what is missing without blocking.

## 9. Documentation and help

- A generated command reference from the schemas (`cargo xtask book`).
- `docs/features.md` rows from the registry (`cargo xtask features`).
- A human guide: **Start a product (or skip it)**, with a worked example, the fields, and what to do
  when a field is unknown.
- Help entries for each field, in `assets/locales/pending/intent.json`, with examples.
- Templates are documented and original; no third-party product plans.
- The help page is reachable from the INTENT tab, the command palette and the MCP tool description.
- The first paragraph of the guide says: "You do not have to fill this in."

## 10. Readiness and validation

There is no gate. When the user chooses to use the brief, the tool validates what they entered:

- A must requirement without an acceptance test is invalid **in the brief editor**; the user can
  skip the requirement or leave it as `should`.
- A budget without a unit is invalid **in the brief**.
- A high risk without a mitigation is allowed; the readiness list suggests adding one.
- A triggered compliance topic without a note is allowed; the readiness list suggests adding one.
- A requirement whose link target is missing shows as a broken reference, never silently dropped.

`brief.readiness` reports, for information:

- what exists and what is missing;
- up to three suggested next tools;
- nothing that blocks save, export, simulation, prototype or release.

Skipping the brief creates no debt, no approval and no "incomplete" state. A design with no brief is a
normal, supported way to work.

## 11. Existing hooks

| Need | Existing |
|---|---|
| Product metadata | `document.metadata`, `component.metadata` |
| Numeric values | parameters and expressions |
| Help surface | `ui.help`, the command reference, the book |
| Start page | `ui.start` and its templates |
| Strings | `assets/locales/pending/<slug>.json`, `cargo xtask lists` |
| MCP | `crates/mcp`, `mcp-server`, the command registry |
| CLI | `solidcraft-cli exec` |
| Schemas | `crates/engine/src/schema/` |

No kernel or geometry work is required.

## 12. Testing

- Schema: valid and invalid fields, missing units, empty must requirements, broken links.
- Readiness: every missing item is reported; "not now" changes nothing; a missing brief does not
  block any command.
- Export: Markdown/PDF contains every section and no stale data; JSON round-trips.
- Four surfaces: GUI scenario, CLI transcript, MCP tool call, generated docs presence.
- Templates: each creates a valid brief; the blank template creates an empty, valid brief.
- Skip path: a design with no brief opens, edits, exports and reopens normally.
- Localization: every field has an English string and a pending locale row.
- Undo/save/reopen: the brief survives; a failed edit changes nothing.
- Agents: an MCP client can read the brief resource and cite a requirement when it exists, and
  model normally when it does not.
- No network, no telemetry.

## 13. Open questions

1. **Default.** Is the brief off by default (recommended), or offered by a dismissible start-page
   card?
2. **Where it lives.** The optional product folder, the design, or both?
3. **Templates.** Which three ship first? Proposal: enclosure + board + firmware, mechanical-only,
   electronics-only.
4. **Readiness.** A command and an optional panel, always advisory? No hard stops anywhere?
5. **Browser.** Full brief editing in wasm with the same commands; product files via OPFS/import.
