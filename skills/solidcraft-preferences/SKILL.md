---
name: solidcraft-preferences
description: "Use when a SolidCraft change alters a default behaviour, an existing shortcut or the look users already have, or adds a setting in Preferences. Covers keeping the old behaviour behind a preference and the PR's 'Changes a default' line."
---

# Preferences and defaults

People rely on how SolidCraft behaves today. A change that makes users see or do something different out of the box is a change of a default.

## The rule
- Keep the old behaviour reachable. Add a preference whose old value gives the old behaviour, and test that it still does.
- Fill in the PR template's `Changes a default:` line (`pr-checks.yml` fails a PR description without it, #6442; CONTRIBUTING.md, "Sending a change"):
  - `Changes a default: no`
  - `Changes a default: yes - <what changes>, old behaviour kept behind preference <name>`

## Adding a preference
Settings live in the `preference_settings!` table in `crates/ui-egui/src/prefs.rs`: the profile scope, field, default, page whose reset restores it, optional accepted range for the control channel, and form control. Add one row there.
- Every row starts with `Profile` or `Computer`. Use `Profile` for personal workflow and appearance settings; use `Computer` for device configuration, performance, local paths, programs, permissions and bookkeeping. The macro requires this choice. Profiles and the General page's summary both use it, independently of the setting's page. Keep personal libraries local unless their separate import/export policy changes.
- A new setting that changes nothing by default needs no `yes` on the line; one that changes what users see by default does.
- Give it a label and help text through the catalogs (`skills/solidcraft-ui-text/SKILL.md`).
- A reset on its page restores the default; fields left out of every page keep their values when defaults are restored.
- Control channel: `ui.prefs {...}` reads and sets preferences by name (`docs/control-protocol.md`).
- Test it in a UI scenario or a `*_tests.rs` file next to the setting (`skills/solidcraft-tests/SKILL.md`).

## What counts as a default
Anything a user sees or does differently on first launch after the update: a changed shortcut, camera or navigation behaviour, unit, naming, theme or autosave behaviour, or a tool that appears unasked. A fix that makes the app do what it already claimed is not a default change; say so on the line.
