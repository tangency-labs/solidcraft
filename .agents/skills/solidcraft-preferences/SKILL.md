---
name: solidcraft-preferences
description: "Use when a SolidCraft change alters a default behaviour, an existing shortcut or the look users already have, or when it wants a new option or setting. Covers where a choice goes (the tool's options panel, rarely Preferences), how old designs keep their result, and the PR's 'Changes a default' line."
---

# Defaults, options and Preferences

People rely on how SolidCraft behaves today. A change that makes users see or do something different out of the box is a change of a default.

## Where a choice goes
- **Old designs keep their old result.** When a change would alter what an existing design builds, the design format records the old behaviour as a legacy value, so a design saved before the change opens and rebuilds as it did. New designs get the new behaviour. Do this through a design-format change (`.agents/skills/solidcraft-file-formats/SKILL.md`), not a setting.
- **A choice people switch while they work** (how a tool behaves, a marker style, a snap mode) goes in that tool's options panel on the right, like the Line tool's options (`.agents/skills/solidcraft-shared-controls/SKILL.md`). It never goes in Preferences.
- **Preferences are for rare, app-wide choices** people set once: units, theme, keys, startup behaviour.
- Don't add a preference to keep a disputed or broken behaviour around, or to paper over a problem. Fix the problem. If people disagree about the behaviour, ask on the issue first.

## The PR's line
Fill in the template's `Changes a default:` line (`pr-checks.yml` fails a PR description without it, #6442; CONTRIBUTING.md, "Sending a change"):
- `Changes a default: no`
- `Changes a default: yes - <what changes>, old behaviour kept behind setting <name>`, where `<name>` is the tool option or the legacy value in the format. The check reads this exact form.

A `yes` waits for a maintainer's decision.

## Adding a preference (rarely)
Settings live in the `preference_settings!` table in `crates/ui-egui/src/prefs.rs`: the field, its default, the page whose reset restores it, an optional accepted range for the control channel, and the form control. Add one row there.
- Give it a label and help text through the catalogs (`.agents/skills/solidcraft-ui-text/SKILL.md`).
- A reset on its page restores the default; fields left out of every page keep their values when defaults are restored.
- Control channel: `ui.prefs {...}` reads and sets preferences by name (`docs/control-protocol.md`).
- Test it in a UI scenario or a `*_tests.rs` file next to the setting (`.agents/skills/solidcraft-tests/SKILL.md`).

## What counts as a default
Anything a user sees or does differently on first launch after the update: a changed shortcut, camera or navigation behaviour, unit, naming, theme or autosave behaviour, or a tool that appears unasked. A changed default is when something people already rely on in the UI changes for everyone: an existing control, interaction or setting value. Not defaults: bug fixes and more correct results, kernel or geometry result changes (write `Changes a default: no`), new behaviour on an input that did nothing before, new options that are off by default, and CI or tooling config. A fix that makes the app do what it already claimed is not a default change; say so on the line.
