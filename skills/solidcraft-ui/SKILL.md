---
name: solidcraft-ui
description: "Use when adding or changing a SolidCraft dialog, panel, menu, overlay, canvas, camera input, slider, icon or colour in crates/ui-egui. Covers the single UI container, the UI kit, navigation::handle, theme tokens, command icons and UI scenarios."
---

# Dialogs and UI

The UI is thin: it reads engine state and acts through `app.run(id, params)` or `app.start(id)`. A dialog is a form that builds the parameters of a command (`skills/solidcraft-commands/SKILL.md`). The full control list is `docs/ui-kit.md`.

## One outer container
- All panels, floating sheets, menus, overlays and platform UI windows use `ui_kit::Container`, directly or through `Panel`, `Window`, `Confirm` and `Menu`. Declare the semantic `Surface`; workspace panels use its shared layout policy and chrome.
- System and report windows use `ui_kit::Window`; its low-level host builder is kit-private.
- Do not construct raw egui hosts, outer popup or window frames, or shadows in feature modules.
- `cargo xtask ui-containers` checks source and examples in all current and future crates and apps, recursively. It runs in the fast repository CI job and in `cargo xtask ci`, with no caller allowances. The inventory and the legitimate content and native exceptions are in `docs/ui-container-audit.md`.

## Controls and colour
- Colours, typography, dimensions and spacing come from `theme::Tokens` (`crates/ui-egui/src/theme.rs`). No literal colours in feature code.
- Sliders only through `ui_kit::slider`. Form rows use `row`, `expr_row`, `text_row`, `choice_row` and `count_row` from `crate::ui_kit` (`crates/ui-egui/src/ui_kit/`).
- Confirmations use `ui_kit::Confirm`; the old `Prompt` element is gone.

## One set of camera controls
Every page or canvas with a camera (viewport, Render preview, a flat pattern or drawing page) calls `navigation::handle` (`crates/ui-egui/src/navigation.rs`) with its `NavCaps` (orbit off for flat 2D pages). Never hand-roll camera input from drags, the wheel or pinch; `hand_rolled_camera_input_is_rejected` fails if a page does.

## Icons
Command icons are original SVG masters compiled through `crates/ui-egui/src/icon_svg.rs`. Read `assets/AGENTS.md`, `assets/icons/ui/AGENTS.md` and `assets/icons/ui/STYLE_GUIDE.md` first, add the ATTRIBUTION.md row, and run `cargo xtask icons` and `cargo xtask assets`. See `skills/solidcraft-ground-rules/SKILL.md`.

## Text
Every label, tooltip and message goes through the catalogs: `skills/solidcraft-ui-text/SKILL.md`. A new behaviour that is on by default: `skills/solidcraft-preferences/SKILL.md`.

## Look at the result
Run the app, drive it and read the screenshot: `skills/solidcraft-agent-tools/SKILL.md`. A UI scenario (`crates/ui-egui/tests/scenarios/*.json`) covers the clicks; see `skills/solidcraft-tests/SKILL.md`. Work that takes long must not block the window: `skills/solidcraft-performance/SKILL.md`.
