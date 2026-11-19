---
name: solidcraft-repro-first
description: "Use before fixing a bug that happens only on some machines: an operating system, desktop or window system, graphics backend or driver, display scaling, or the updater and installer. Covers the evidence a fix needs and how to keep everyone else's path unchanged."
---

# Platform, graphics, driver and updater fixes: reproduce first

These bugs show up on one kind of machine and not on yours. A fix written from a guess often changes the path that works for everyone else. So the evidence comes first.

## Before you write code
- Get a recording or the log from a **released build** (a release or a nightly, with its version) on the machine where it fails. A build of your own branch doesn't count: it is not what users run.
- Record the configuration: the operating system and version, the graphics backend, the GPU and driver version, the display scaling, and how the app was installed.
- Say what the user does, what they expect and what happens. Strip user names, home folders and private designs from logs (`skills/solidcraft-reporting/SKILL.md`).
- No evidence yet: ask for it on the issue, and don't open a speculative fix.

## The fix
- Scope it to the condition you saw: detect that operating system, backend, driver or setting, and change behaviour only there. Name the condition in the code and in the PR.
- The default path stays as it is for every other machine. If the fix would change what every user gets, it is a changed default (`.agents/skills/solidcraft-preferences/SKILL.md`), and it needs a maintainer's yes first.
- Don't add a preference so users can work around the bug. Fix the bug.
- Don't swallow the error to hide it. If the app can't do something on that machine, it says so and goes on with what it can.
- Never crash (`.agents/skills/solidcraft-crash-safety/SKILL.md`), and keep work off the window thread (`.agents/skills/solidcraft-performance/SKILL.md`).

## Proving it
- Show the same check on the affected machine with a build of the PR (CI's build artifact, or one you made), and put the recording or log in the PR.
- If you can't reach that machine, say so in the PR and ask the reporter to try the PR's build. Don't call it fixed before they have.
- Show that the path everyone else uses still works: run the app on a machine without the condition, and say which tests cover it.

## The PR body
- The configuration it fixes, with the evidence from the released build.
- The condition the fix checks, and why other configurations don't reach the new code.
- `Changes a default: no` when the fix only makes the affected configuration work as intended. If it also changes a shortcut or behaviour users there already rely on, that is `yes`.
