---
name: solidcraft-reporting
description: Use when a SolidCraft user says something went wrong - the app crashed or closed, a command failed or gave the wrong shape, a number is off, a design no longer rebuilds, something hangs - and wants it reported or fixed upstream. Turns the problem into a minimal, reproducible bug report with exact steps, a command script, expected and actual results and a screenshot.
---

# SolidCraft reporting

A report the maintainers can act on reproduces in a fresh design, in as few steps as possible, with the exact message and numbers. Build that here, so nobody has to rebuild the problem from a description. Use `reference.md` from `solidcraft-modeling` for command shapes, and `reference/report-template.md` (next to this file) for the report.

Written against SolidCraft 0.18.2. Report what you observed, never a guessed cause: "the fillet fails with `…`", not "the kernel can't handle tangent edges".

## 1. Gather (ask only for what you cannot find)
- **Version and OS.** Help → About in the app, `solidcraft --version` or `solidcraft-cli --version`; the MCP server reports its version in `initialize` (`serverInfo.version`). Note whether the user built from source.
- **What the user did**, in order, what they expected and what happened instead.
- **The exact error text**, copied, not paraphrased: a failed `execute` or `batch` step's `failed.error`, the timeline feature's `error` in `inspect_design {"measure":false,"sketches":false}`, or the message in the app.
- **If the app closed or froze:** `crash_reports {}` first. A matching report has the version, the command that crashed, the commands before it and a replay. `solidcraft-cli crashes replay <id>` checks that it still happens. Its `design.sc` is the user's design: see Privacy.

## 2. Reproduce in a fresh design
Start from nothing, so the report does not depend on the user's file.
- **Headless:** write the steps as a command script `{"commands":[...]}` and run `solidcraft-cli run repro.json`. A failing feature fails the run and names the feature. This never touches the design open in the app.
- **Over MCP:** `new_design {}`, then the steps in one `batch {"commands":[...],"rollback":false}`. With `rollback: false` the steps before the failure stay, so you can inspect the state it failed in. When the server is bridged to the app and the open design is `dirty`, ask before `new_design`.
- Find commands and parameters with `list_commands {"filter":"fillet"}` and `describe_command {"id":"solid.fillet"}`, not the unfiltered catalog.
- **Only in the user's design:** `solidcraft-cli script design.sc --out repro.json` writes its history as a command script. `solidcraft-cli eval repro.json` replays it and prints the measurements; `solidcraft-cli run repro.json` stops at the first failing feature. If the script rebuilds without the problem, say so in the report: it then depends on something the history does not hold (an import, a file the design links to).
- **Interactive only** (a dialog, a pick, a drag, the viewport): write the clicks as numbered app steps. A script cannot show those.
- Check that it fails the same way twice. A problem that comes and goes is still worth reporting; say how often it happened.

## 3. Minimise
Shrink while the same failure, with the same message, still happens. Re-run after every change:
1. Drop steps from the end, then from the middle: other bodies, features after the failing one, cosmetic steps (appearance, names, views).
2. Replace sketches with `solid.box` or `solid.cylinder` where the failure survives it.
3. Round the dimensions (10, 20, 2.5) and replace parameter expressions with numbers, unless an expression is the problem.
4. Stop when removing anything more makes it pass. Note the boundary when you find one ("fails at radius 5, works at 4.9").

**Hangs and slow commands:** never wait forever. Cap every run: `timeout 120 solidcraft-cli run repro.json --timings` prints the milliseconds of each step when the run finishes. A run that is killed prints nothing, so find the slow step by running the script one step longer at a time. Over MCP, `batch {"commands":[...],"timings":true}` reports every step's time, the failing one included. Report the step, the time, and how the time grows when you double a count or a size.

## 4. Check the claim
- Wrong geometry: `measure {}` (volume, area, bounding box, centre of mass) against a value you computed by hand. Put both numbers in the report.
- Wrong pick or wrong face: `body_topology {"body":"Body1"}` shows what was there to pick.
- A look at it: `screenshot {"view":"iso"}` after the `rollback: false` batch, or `solidcraft-cli snapshot repro.json --out shot.png --view iso`. `snapshot` fails with the script when a step fails, so for a failing command snapshot the script without that step and show the state it fails on. Look at the image before you attach it.
- Re-run the final minimal script from scratch once more. Its output goes into the report as it is.

## 5. Privacy
- Never attach, upload or quote the user's design, a crash report's `design.sc`, or file contents without asking. The minimal synthetic script is the default attachment.
- GitHub does not accept `.sc` attachments. A design the user agrees to share goes up as a `.zip`: a `.sc` file is a ZIP already, so copy or rename it to `name.sc.zip`, or zip it. SolidCraft opens either kind of `.zip`.
- Strip home folders, user names, part and project names and file paths from messages and logs (`/home/alice/clients/acme/bracket.sc` → `bracket.sc`).
- Check screenshots for other windows, file names and anything personal.

## 6. Search, then write
- Search before writing: `gh issue list -R tangency-labs/solidcraft --state all --search "fillet radius"` with two or three words from the error and the command. If one matches, add the minimal script and your version to it as a comment, instead of filing a new one.
- Write the report with `reference/report-template.md`. Title `<Area>: <what fails>`, such as `Fillet: a radius above half the edge's face fails with "…"`.
- Show the user the finished report. File it only after they approve: `gh issue create -R tangency-labs/solidcraft --label bug --title "…" --body-file report.md`, or let them paste it into **Report a bug** in the app's title bar, which adds a screenshot they review first.

## Traps
- **The wrong version.** The CLI on `PATH` can be older or newer than the app the user ran. Reproduce with the user's version and say which one you used.
- **Fixed already.** If it does not reproduce on the latest release or `main` but does on the user's version, say so: it saves a report.
- **Edge picks** snap to the nearest edge with no distance limit and no error, so a repro can pick a different edge than the user did. Check the face count or `body_topology` after the step.
- **A success can be the bug.** A cut that misses its body still reports success. Measure, don't trust the reply.
