---
name: solidcraft-commands
description: "Use when adding or changing a SolidCraft command or any user-visible behaviour, or when deciding which crate code belongs in. Covers CommandSpec, command ids, programmatic calls vs dialogs, crate layering and the generated feature list."
---

# Adding a command

User-visible behaviour is a command. The toolbar, dialogs, sketch tools, palette, scripts, CLI, control channel and MCP all reach the same commands through `Session::execute(id, params)`. The long form is `docs/book/src/dev/commands.md`.

## Steps
1. Add a `CommandSpec` to the right file in `crates/engine/src/cmd/` and implement `run` (`fn(&mut Session, &Value) -> Result<Value>`). Fields: id, label, toolbar tab and panel (`.at`), icon (`.icon`), shortcut (`.key`), params doc (`.params`), `enabled` (its error string is the disabled-button tooltip).
2. The id is our own: dotted, lower-case `<area>.<thing>[.<variant>]`, such as `solid.extrude` or `sketch.rectangle.two_point`. Never another product's id.
3. Validate inputs first (units, ranges, references) and return `BadParams` rather than building a broken feature. Parameters are hostile: see `skills/solidcraft-crash-safety/SKILL.md`.
4. Give it input and output schemas: a contract and a reply type in `crates/engine/src/schema/`, registered with `checked!`, `.contract` and `.output`. A command should have them and a test warns without them; this will become required. `schema/unconverted.txt` only shrinks. How: `docs/book/src/dev/schemas.md`.
5. Engine tests measure the result: volume, area, face/edge/vertex counts against values you can derive by hand. Where they go: `skills/solidcraft-tests/SKILL.md`.
6. Add the label as `cmd.<command id>` in a pending locale file (`cargo xtask i18n --add-missing` lists what is missing); a test fails without it. See `skills/solidcraft-ui-text/SKILL.md`.
7. A command the ribbon shows also needs help: `help.<command id>` (what it does, then when to use it) in the pending file, and its id in `DOCUMENTED` in `crates/ui-egui/src/command_help.rs`. A test fails without them.
8. Run `cargo xtask features` to see the generated `docs/features.md` change, but do not commit it: `main` regenerates it after each merge.
9. If the command saves something new in a design, add a format file: `skills/solidcraft-file-formats/SKILL.md`.
10. If it needs a dialog or panel: `skills/solidcraft-ui/SKILL.md`.

## Programmatic calls never open dialogs
`engine.execute` runs the JSON form. Only toolbar clicks (`ui.start`) start interactive tools and dialogs, which then run commands. A dialog is a front-end form that builds the parameters.

A command that changes the design is undoable by default (the session snapshots the document first), and a failing command leaves the document unchanged.

## Layering
`cargo xtask layers` fails the build when a lower layer depends on a higher one:

L0 `geom` -> L1 `sketch`, `kernel`, `render` -> L2 `doc` -> L3 `io` -> L4 `engine` -> L5 `ui-egui` -> apps.

- Nothing below L5 depends on egui, eframe, winit, wgpu or rfd. The UI crate is swappable.
- Only `crates/kernel` may use `truck`.
- The UI is thin: panels read engine state and act through `app.run(id, params)` or `app.start(id)`.
- A new module's `mod` line goes into the existing sorted group, not a new group at the end (`cargo fmt` keeps each group sorted).

## Roadmap
`ROADMAP.md` tracks milestones; update it when a milestone task lands.
