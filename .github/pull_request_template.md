<!-- Title: which part of the app changes and what a user now sees or can do, clear to someone who hasn't seen your chat, the issue or the code. -->

## Change

Start with what this is, in plain words someone new to the project would understand: what it does and who it's for. Don't assume anyone knows the name of your tool, project or feature. Then describe the problem and resulting behavior. Link the issue when applicable.

Changes a default: no

<!-- Required line, replace "no" if this changes what a user gets out of the box (a default value, key binding, setting, tool behaviour; a UX default only, a kernel result change is 'no'):
Changes a default: yes — <what>, old behaviour kept behind preference <name>
Docs-only and bot PRs are exempt. -->

## Validation

What did you run, and what were the results? Include expected and measured values for geometry changes.

## Screenshots of the behavior (when observable)

Put the picture or video here in the description. A file in the diff doesn't count: a maintainer has to be able to see what this is without opening the changed files.

For observable CAD, UI or model behavior, include actual descriptive screenshots or a short recording from a small reproduction. This includes kernel, geometry, assembly and backend effects that can be shown in the app. Coding agents should run the app and capture the evidence themselves, rather than asking a human for evidence they can produce. Show the actual failure/result, preferably before and after, with brief steps, expected/actual behavior and annotations where helpful.

Flowcharts and Mermaid diagrams cannot substitute for evidence of visible behavior. Use diagrams for architectural reasoning when actual screenshots cannot illustrate the point. Changes with no useful observable behavior may omit screenshot/video evidence. Exclude private designs and sensitive details; attach media or link to an accessible recording rather than committing generated review artifacts.

## Integration status

Record the upstream commit and PR status checked before handoff, any conflicts resolved, and validation after integration. Keep up with upstream merges and monitor this PR while the task is active; rebase before pushing and never force-push.
