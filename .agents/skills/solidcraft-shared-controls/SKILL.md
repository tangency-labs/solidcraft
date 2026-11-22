---
name: solidcraft-shared-controls
description: "Use when adding or changing how a SolidCraft tool, workspace or page handles selection, picking, keys, Esc, Delete, undo or the tool's start and finish, or when a change wants a new option or setting. Covers the shared controls and where runtime choices go."
---

# Shared controls

Every workspace and tool behaves like the rest of the app. People learn the controls once. A control that works differently in one place is a bug, however good the idea.

## Use the shared pieces
- **Selection and picking:** the shared selection model, preselection, box select (window and crossing) and picking in `crates/ui-egui/src/selection.rs`. Never a second picker for one workspace.
- **Keys:** shortcuts come from the keymap (`crates/ui-egui/src/keymap.rs`), where users can rebind them. Delete, Backspace, Esc and undo/redo do what they do everywhere else. Don't read keys yourself for things the keymap or the app already handles.
- **Tool lifecycle:** a tool starts, shows its options, takes picks, applies or cancels, and Esc leaves it, the same way as the sketch tools (`crates/ui-egui/src/tools.rs`). The work is a command (`.agents/skills/solidcraft-commands/SKILL.md`).
- **Camera:** every page with a camera calls `navigation::handle` (`crates/ui-egui/src/navigation.rs`); see `.agents/skills/solidcraft-ui/SKILL.md`.
- **Dimensions and editing values** work the way they do in a sketch.
- If a shared piece lacks what you need, extend it for everyone. Don't copy it into a private version.

## Where a choice goes
- A choice people switch while they work (how a tool behaves, a marker style, a snap mode) goes in **that tool's options panel** on the right, like the Line tool's options. It never goes in Preferences.
- Preferences hold settings people set once: units, theme, keys, startup behaviour.
- Don't add a preference to paper over a problem. If a behaviour is wrong, fix it. If people disagree about it, ask on the issue first.
- A new option, row, icon or menu entry in an existing dialog, toolbar or menu needs full-flow screenshots at 1280×720 (`.agents/skills/solidcraft-file-pr/SKILL.md`). Prefer no new option at all when the tool can choose well by itself.

## Defaults
Changing what users get out of the box in an existing control is a changed default: the existing behaviour stays the default unless a maintainer decides otherwise. Old designs keep their old result through a legacy value in the format, and the PR fills in `Changes a default:` (`.agents/skills/solidcraft-preferences/SKILL.md`). New behaviour on an input that did nothing before is not a changed default.

## Check it
Drive the change in the running app as a user would: start the tool, pick, press Esc, Delete and undo, and switch workspace and back. Compare it with the same steps in the model and sketch workspaces. A UI scenario (`crates/ui-egui/tests/scenarios/`) keeps it that way (`.agents/skills/solidcraft-tests/SKILL.md`).
