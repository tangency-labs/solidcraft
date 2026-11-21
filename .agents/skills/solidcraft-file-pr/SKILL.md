---
name: solidcraft-file-pr
description: "Use when opening or updating a SolidCraft pull request: the title, the body, the template lines the PR checks read, and the screenshots a UI change needs."
---

# Filing a pull request

Adapted from tstack's file-pr (https://github.com/t3dotgg/tstack, MIT; see `.agents/skills/LICENSE-tstack`).

## Before you open it
- Look for an open PR on the same issue or files (`.agents/skills/solidcraft-contributing/SKILL.md`). The lowest-numbered one wins.
- Read your diff against `origin/main`: one topic; no `plan/`, `target*/`, generated or media files. A docs-only PR touches no code file, not even a comment.

## The title
It becomes the commit on `main`. Form: `Area: what the user or caller now gets, in plain words`.
- Area: what people recognise (Sketch, Drawings, Fillet, STEP import, Plugins, solidcore, Kernel, CI, Docs), not a crate or file.
- Behaviour, not mechanism, in the present tense. For a fix, say what now works, not "fix bug".
- A refusal says what is refused and why, in a few words.
- No issue number (the merge adds `(#N)`), no `feat:`/`fix:`, no emoji, no "WIP" (open a draft). Under about 100 characters, important words first.

From merged titles:
- Good: `Shell: a feature narrower than twice the wall stays solid instead of refusing the shell`
- Good: `Plugins: a plugin keeps its own data in a design, and the design keeps it when the plugin is missing`
- Good: `Chamfer: an unequal chamfer too large for its faces is refused with the scale that fits, not a blend failure`
- Bad: `fix(kernel): allow single-face open sheet to split body`. Better: `Split Body: an open sheet of one face splits a body`
- Bad: `feat(parts): let plugins seat custom screws in picked bores`. Better: `Plugins: a plugin can seat its own screws in the bores you pick`
- Bad: `Quick menu for the selection`. Better: `Selection: a quick menu of what you can do with the selection`

## The body
Open with the problem as a user meets it, then the result. Don't list the functions you changed. Write in your own words: no chat transcripts, chat links or tool footers. Fill in `.github/pull_request_template.md`:
- `Fixes #N` when there is an issue.
- `Changes a default: no`, or `Changes a default: yes - <what>, old behaviour kept behind setting <name>` (the tool option or the format's legacy value). UX only: a kernel result, or new behaviour on an input that did nothing, is `no` (`.agents/skills/solidcraft-preferences/SKILL.md`).
- `Test change: <reason>` when the diff changes or deletes a line of an existing test or golden: why the old value was wrong, derived (`.agents/skills/solidcraft-kernel-rules/SKILL.md`). New tests need no line.
- Validation: what you ran and what came out; expected and measured values for geometry.

## Pictures
- Screenshots or a short recording in the description, captured yourself from a small public example. A file in the diff doesn't count.
- The whole flow, uncropped: ribbon tab and button, each dialog, the picks, the result; before and after is best.
- A UI addition (a row, option, icon, button or entry in an existing dialog, toolbar, ribbon or menu) needs full-flow screenshots at 1280×720 and a small-window check.
- Diagrams don't replace pictures. Check pictures and logs for private designs, home folders, user names and tokens.

## Links and opening
Name every PR and issue with a few words and its full link. Open it ready for review once the change is done (a claim is the one draft case), then follow `.agents/skills/solidcraft-babysit-pr/SKILL.md`.
