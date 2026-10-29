# SolidCraft: instructions for agents

SolidCraft is a clean-room, pure-Rust parametric 3D CAD app (sketch solver, feature timeline, B-rep kernel, an egui UI that agents drive). This file is a router: the hard rules, then the skill to read for your task. Humans: CONTRIBUTING.md.

## Start here
Work: ROADMAP.md, "Where help is wanted most" in CONTRIBUTING.md, issues labelled `help wanted`, `good first issue`, `needs-triage`; architecture in `docs/architecture.md`. Act when the code, docs and rules settle a question; ask only when it changes what users see. Local `plan/` notes are optional.

## Hard rules (what enforces each)
- Independent: never copy another product's code, icons, text or templates; never drive or take data from commercial CAD. `skills/solidcraft-ground-rules/SKILL.md`.
- No GPL/LGPL/AGPL code, not even reading it (OpenCASCADE, FreeCAD, SolveSpace, CadQuery, LibreCAD, QCAD): `cargo xtask licences --check`.
- Every asset is original or openly licensed with a row in ATTRIBUTION.md; read `assets/AGENTS.md` before icon work: `cargo xtask assets`, `cargo xtask icons`.
- Never crash: no unwrap/expect/panic/unsafe in non-test code (unsafe allowlist in crash-safety), errors are `Result`, input is hostile: clippy `-D warnings`, `hostile_params_never_panic`.
- Everything is a command and programmatic calls never open dialogs: `skills/solidcraft-commands/SKILL.md`.
- Layering (geom, sketch/kernel/render, doc, io, engine, ui-egui, apps); only `crates/kernel` uses truck: `cargo xtask layers`.
- One outer UI container, one camera path (`navigation::handle`), colours from theme tokens: `cargo xtask ui-containers`.
- Application code is Rust only and builds for wasm: `cargo xtask wasm`, `cargo xtask web-offline-test`.
- The product name is SolidCraft; the old name lives only in `docs/compatibility.md`.
- Sorted lists (message catalogs, `mod` lines, languages) stay sorted: `cargo xtask lists`. Never edit generated files (`docs/features.md`, `THIRD-PARTY-LICENSES.txt`) in a PR.
- A design-format change is ONE new file in `crates/doc/src/format/pending/`: `scripts/test-format-pending.sh`.
- A command has input and output schemas (will become required; `schema/unconverted.txt` only shrinks): `skills/solidcraft-commands/SKILL.md`.
- New UI strings go in `assets/locales/pending/<slug>.json`, ATTRIBUTION rows in `attribution/pending/<slug>.md`: `cargo xtask lists`.
- A changed default keeps the old behaviour behind a preference and fills the PR's `Changes a default:` line, and no second PR for an issue a lower-numbered one closes: `pr-checks.yml`.
- A regression test goes in the area's existing test file, never a per-bug file (#3226).
- Look at open PRs first, claim with a draft PR (lowest number wins), one topic per PR, never force-push `main`, no pushes just to catch up with `main`: `skills/solidcraft-contributing/SKILL.md`.
- Show observable changes with real screenshots or a recording, captured yourself.
- Run `cargo xtask ci` before every commit, without switching branches while it runs. `cargo xtask skills` keeps this file and `skills/` honest.

## Working on X: read the skill first
| Working on | Read |
|---|---|
| A PR, a claim, review, CI or evidence | `skills/solidcraft-contributing/SKILL.md` |
| Licences, assets, clean-room, the name | `skills/solidcraft-ground-rules/SKILL.md` |
| Non-test Rust, hostile input, a crash report | `skills/solidcraft-crash-safety/SKILL.md` |
| A command, a crate boundary | `skills/solidcraft-commands/SKILL.md` |
| A dialog, panel, camera, icon, colour | `skills/solidcraft-ui/SKILL.md` |
| Labels, messages, translations | `skills/solidcraft-ui-text/SKILL.md` |
| Saving something new in a design, import, export | `skills/solidcraft-file-formats/SKILL.md` |
| Preferences, a changed default | `skills/solidcraft-preferences/SKILL.md` |
| Writing or running tests | `skills/solidcraft-tests/SKILL.md` |
| Slow work, the window thread, wasm | `skills/solidcraft-performance/SKILL.md` |
| Releases, installers, packages | `skills/solidcraft-packaging/SKILL.md` |
| Running the app, control channel, MCP tools | `skills/solidcraft-agent-tools/SKILL.md` |
| Building a part over MCP | `skills/solidcraft-modeling/SKILL.md` |
| Changing an existing design | `skills/solidcraft-editing/SKILL.md` |
| A bug report, a crash, a wrong result | `skills/solidcraft-reporting/SKILL.md` |
| Checking, porting, designs in git | `skills/solidcraft-checking/SKILL.md` |
| Assemblies, joints, motion | `skills/solidcraft-mechanisms/SKILL.md` |
