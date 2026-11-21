# SolidCraft: instructions for agents

SolidCraft is a clean-room, pure-Rust parametric 3D CAD app (sketch solver, feature timeline, B-rep kernel, egui UI). This file is a router: the hard rules, then the skill for each task. Humans: CONTRIBUTING.md.

## Start here
Work: ROADMAP.md, "Where help is wanted most" in CONTRIBUTING.md, issues labelled `help wanted`, `good first issue`, `needs-triage`; architecture in `docs/architecture.md`.

## Hard rules (what enforces each)
- Integrations with other platforms need the maintainer's yes first: `.agents/skills/solidcraft-packaging/SKILL.md`.
- Independent: never copy another product's code, icons, text or templates; never drive or take data from commercial CAD. `.agents/skills/solidcraft-ground-rules/SKILL.md`.
- No GPL/LGPL/AGPL code, not even reading it (OpenCASCADE, FreeCAD, SolveSpace, CadQuery, LibreCAD, QCAD): `cargo xtask licences --check`.
- Every asset is original or openly licensed with a row in ATTRIBUTION.md; read `assets/AGENTS.md` before icon work: `cargo xtask assets`, `cargo xtask icons`.
- Never crash: no unwrap/expect/panic/unsafe in non-test code, errors are `Result`, input is hostile: clippy `-D warnings`, `hostile_params_never_panic`.
- Model decisions (what an operation does, which bodies it touches, which face or edge a point or name means, depths, nearness) use exact geometry; a mesh is for display, export, previews and mesh bodies (`xtask/mesh-decisions.txt` only shrinks): `cargo xtask mesh-decisions`.
- Everything is a command and programmatic calls never open dialogs: `.agents/skills/solidcraft-commands/SKILL.md`.
- Layering; only `crates/kernel` uses truck; solidcore only via `solidcore-ops`/`-tess`/`-record` (`xtask/solidcore-seam.txt`): `cargo xtask layers`; see `docs/layers.md`.
- One outer UI container, one camera path (`navigation::handle`), colours from theme tokens: `cargo xtask ui-containers`.
- Application code is Rust only and builds for wasm: `cargo xtask wasm`, `cargo xtask web-offline-test`.
- The product name is SolidCraft; the old name lives in `docs/compatibility.md`.
- Lists (message catalogs, `mod` lines, languages) stay sorted: `cargo xtask lists`. Never edit generated files (`docs/features.md`, `THIRD-PARTY-LICENSES.txt`) in a PR.
- A design-format change is ONE new file in `crates/doc/src/format/pending/`: `scripts/test-format-pending.sh`.
- A command has I/O schemas (soon required; `crates/engine/src/schema/unconverted.txt` only shrinks): `.agents/skills/solidcraft-commands/SKILL.md`.
- New UI strings go in `assets/locales/pending/<slug>.json`, ATTRIBUTION rows in `attribution/pending/<slug>.md`: `cargo xtask lists`.
- A changed UX default (not kernel results): old designs keep their result via a format legacy value, runtime choices go in the tool's options panel; fill `Changes a default:`.
- A regression test goes in the area's existing test file, never a per-bug file (#3226).
- Look at open PRs first, claim with a draft PR (lowest number wins), one topic per PR, English, a title a newcomer understands, screenshots you made: `.agents/skills/solidcraft-contributing/SKILL.md`.
- Run `cargo xtask fast` before each commit; CI runs the full gate. `cargo xtask skills` checks this file and both skill folders.

## Skills: read the one for your task
Dev skills: `.agents/skills/` (`.claude/skills` links there; Windows: Developer Mode, `git config core.symlinks true`). In-app MCP skills: `skills/`.

| Skill | When |
|---|---|
| `.agents/skills/solidcraft-contributing/SKILL.md` | a fork, a claim, every PR's rules |
| `.agents/skills/solidcraft-file-pr/SKILL.md` | opening a PR |
| `.agents/skills/solidcraft-babysit-pr/SKILL.md` | after a push: CI, review |
| `.agents/skills/solidcraft-full-stop/SKILL.md` | a change end to end |
| `.agents/skills/solidcraft-takeover/SKILL.md` | finishing someone else's PR |
| `.agents/skills/solidcraft-modernize/SKILL.md` | an old PR |
| `.agents/skills/solidcraft-ground-rules/SKILL.md` | licences, assets, the name |
| `.agents/skills/solidcraft-crash-safety/SKILL.md` | non-test Rust, crashes |
| `.agents/skills/solidcraft-commands/SKILL.md` | a command, a crate boundary |
| `.agents/skills/solidcraft-layers/SKILL.md` | SolidCraft or solidcore |
| `.agents/skills/solidcraft-kernel-rules/SKILL.md` | geometry, test expectations |
| `.agents/skills/solidcraft-ui/SKILL.md` | a dialog, panel, camera, icon, colour |
| `.agents/skills/solidcraft-shared-controls/SKILL.md` | selection, keys, tool options |
| `.agents/skills/solidcraft-repro-first/SKILL.md` | OS, graphics, updater fixes |
| `skills/solidcraft-sheetmetal/SKILL.md` | sheet metal |
| `.agents/skills/solidcraft-ui-text/SKILL.md` | labels, messages, translations |
| `.agents/skills/solidcraft-file-formats/SKILL.md` | design saving, import, export |
| `.agents/skills/solidcraft-preferences/SKILL.md` | a changed default, a new option |
| `.agents/skills/solidcraft-tests/SKILL.md` | writing or running tests |
| `.agents/skills/solidcraft-performance/SKILL.md` | slow work, the window thread, wasm |
| `.agents/skills/solidcraft-packaging/SKILL.md` | releases, installers, packages |
| `.agents/skills/solidcraft-agent-tools/SKILL.md` | running the app, MCP |
| `skills/solidcraft-modeling/SKILL.md` | building a part over MCP |
| `skills/solidcraft-editing/SKILL.md` | changing an existing design |
| `skills/solidcraft-reporting/SKILL.md` | a bug report |
| `skills/solidcraft-checking/SKILL.md` | checking, porting, designs in git |
| `skills/solidcraft-mechanisms/SKILL.md` | assemblies, joints, motion |
