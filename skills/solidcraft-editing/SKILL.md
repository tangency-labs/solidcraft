---
name: solidcraft-editing
description: "Use when changing, fixing or extending an existing SolidCraft design through its MCP server - a .sc file (or an older .solidcraft one) or the design open in the app. Typical requests: resize it, move a cut, change a hole or standoff, add a fillet or chamfer, make a fixed value parametric, or a feature shows an error after a change or after the app crashed."
---

# SolidCraft editing

## Overview
Edit an existing design through its parameters instead of rebuilding it. Every sketch dimension and feature input is already a parameter (`d1`, `d2`, ...), so most edits are a `parameters.change` each, sent together in one batch. Map the design with a few compact calls rather than walking the timeline one feature at a time. For command shapes and traps, in SolidCraft's own chat or any MCP client, call `skill_reference {"skill":"solidcraft-modeling"}`. Elsewhere, use `reference.md` from the `solidcraft-modeling` skill.

## Map the design (3-4 calls)
1. **Open:** if the design is not open yet, run `open {path}`. When the server is bridged to the app and the current design is `dirty`, ask before opening anything.
2. **Overview:** `inspect_design {"measure":false,"sketches":false}` lists the user parameters, the sketch dimensions (`d1`, `d2`, ... with their expressions), the features in order, and their errors.
3. **Feature inputs:** `execute parameters.list {"filter":"feature"}` gives every feature input as an object such as `{"name":"d9","expression":"(6) / 2","feature":"Cylinder1","input":"Radius"}`. `inspect_design` leaves these out.
   - Match sketch dimensions by expression.
   - If that is ambiguous, `sketch.inspect {sketch}` that one sketch, which also shows its points in world coordinates.
4. **Baseline:** `measure {}`, so you can predict every change against it.

`timeline.inspect {feature}` is only for two cases:
- changing more than a number: profiles, direction, operation or edges
- checking whether a position follows a parameter, for example a cylinder `base` written as `width/2 - wall - 6`

Inspect one feature of each kind, not every copy.

## Change
- **A number:** `parameters.change {"name":"d9","expression":"..."}`.
  - **To make a fixed value parametric:** add a user parameter first (`parameters.add standoff_d`), then set each input to an expression of it (`standoff_d/2`).
  - **Several changes:** one `batch`.
- **A user parameter:** `set_parameter` or `parameters.change`. Every expression that uses it follows.
- **Structure:** `timeline.edit {"feature":..., "set":{...}}`, with the field shapes `timeline.inspect` shows.
- **A new feature** goes at the end of the timeline. To place it earlier:
  1. `timeline.roll_to {"position":N}`. A position is safer than a name, since a sketch and a feature can share one.
  2. Add the feature.
  3. `timeline.roll_to {}` to return to the end.

  Everything after the insert recomputes, and a feature that worked can now fail. Seen: a corner fillet after an inserted chamfer.
- **Risky or multi-step edits:** take `edit.checkpoint` in its own call first. `edit.restore_checkpoint {checkpoint}` goes back exactly. Restoring uses the checkpoint up (and any later ones): take a new one to go back again. Only the last 16 are kept. A `batch` rolls itself back; separate `execute` calls do not.
- **Deleting:** `timeline.dependents {feature}` first. It lists what is deleted with it and what would fail.
- **Ambiguous wording:** "2 mm higher" on a value written as `wall + 6` can be relative (`wall + 8`) or absolute.
  - Keep the design's intent, which is the relative reading.
  - Tell the user which reading you used. This matters most when the same request also changes `wall`, because then the two readings give different heights.
- **New constants** such as a chamfer size go in user parameters, so the user can change them later.

## Linked designs and the values they take
A linked component (Insert Design) can take values from this design: each a user parameter of the linked design set to an expression over this design's parameters.
- **What is linked:** `execute component.links {}`. Each link lists `parameters` (what it passes: `expression`, `value`, `by_name`) and `parameters_kept`.
- **What a design could take:** `execute component.design_parameters {"path": file}` (before inserting) or `{"component": c}`. It lists the linked design's number user parameters, its own `value` of each, and `here`: the value this design would give it by name.
- **Pass values:** `execute component.link_parameters {"component": c, "set": {"h": "lift"}, "by_name": true, "keep": ["w"]}`.
  - `set` adds or changes the link's expressions; `null` removes one. With `by_name` on, a parameter this design has by name then takes that value: to give it back to the design's own value, also name it in `keep` (with the rest of the list, which `keep` replaces).
  - `by_name: true` passes every parameter that shares a name with one of this design's user parameters. An expression in `set` wins.
  - `keep` replaces the list of names left at the design's own value; `[]` clears it. A name in both `set` and `keep` is refused.
  - An option left out stays as it was. Only number user parameters can be set.
  - A parameter that only shares a name but not a kind (an angle here, a length there) keeps its own value; the reply's `warnings` says so, as do those of `component.insert_design` and `component.update_links`.
- **New links:** `component.insert_design` takes the same `parameters`, `by_name` and `keep`. With `reuse` it reuses only a link that passes the same values.
- **After a value edit** the links follow in the same undo step, and so they do after any edit that changes what they are passed (a feature input an expression reads, a rename that gives a by-name link a new match). `parameters.change` replies `linked` (the components that took new bodies) and `link_warnings`: links that kept their bodies, and why. Any other edit whose links followed or kept their bodies (a feature edit, a rename, `parameters.add`) adds the same two keys to its reply. Inside a plugin's run a linked design is read only from the folders the plugin may read; elsewhere its link keeps its bodies. A file that changed since this design read it is taken in by `component.update_links`; a value edit never does. A file that is gone must be put back at its path, or the link given another with `component.relink`, first. When the file renamed or removed a parameter the link sets, Update Links can't take it in with the old settings: set the new ones with `component.link_parameters {..., "update": true}`, which takes the file in with them in one step (`component.design_parameters {"component": c}` says `changed: true` and lists the file's parameters as they are now). A design that links one that links it back is a loop, and the link that closes it keeps its bodies. A link whose design has a link inside that keeps its bodies (at any depth) keeps its own bodies and the values they were made with too; its warning gives the path from this design's placement (`Second keeps its bodies: in middle.sc, Leaf keeps its bodies: …`). Inserting such a design with values, or setting its link's values, is refused and names the link inside, even when the design already has those values (a configuration, or saved that way).
- **Renaming** a parameter rewrites the link expressions that use it, and a parameter a link took by name goes on being passed under its new name. Deleting a parameter an expression uses is refused; `parameters.users` lists the link.
- **Verify** with `measure` after the edit. A warning that says a link keeps its bodies means that component did not change. A component can be in `linked` and have a warning too: it took new bodies, and one parameter kept its own value.

## Verify
1. **Predict, then measure once.** Send the independent changes in one batch, predict the total volume change, then run `measure` once.
   - A move does not change the volume. To confirm a moved sketch, read its points' world coordinates with `sketch.inspect`. You can also predict the COM shift, `Δcom = V_moved · Δz / V_body`, but only when nothing else in that batch changes the volume.
   - If it matches to about 0.01%, you are done.
   - If not, restore the checkpoint, take a new one, then apply the changes one at a time, measuring after each.
   - A cut or chamfer that leaves the volume unchanged has missed.
2. **Timeline health:** run `inspect_design {"measure":false,"sketches":false}`. Every `error` and `warning` must be null. If something broke, restore or `undo` before you try anything else.
3. **Interior features:** take one top-view screenshot, `{"source":"model","view":"top","width":480,"height":320}`. When the server is bridged, `view` also turns the user's camera. The iso view hides standoffs behind the walls.
4. **Still parametric:** if the user wants it kept parametric, change one driving size, `measure`, then set it back.

## When something is already broken
- **A feature has an `error` after opening:** read it with `timeline.inspect`, fix the input or definition, and recompute.
- **The app crashed:**
  - `crash_reports` lists the reports, and `crash_reports {id}` gives the command that crashed.
  - `execute doc.recovery_list` and then `doc.recover {id}` reopen the autosaved design.
  - Never retry the crashing command unchanged.

## Save
`save {"path":"...-v2.sc"}` writes a copy. `save {}` overwrites the opened file; do that only when the user asked for it.

## Red flags

| Thought | Do instead |
|---|---|
| "timeline.inspect each feature to understand it" | `parameters.list {"filter":"feature"}` shows every input in one call. |
| "Delete and remodel the feature" | Change its `dN` input, or use `timeline.edit`. History and downstream picks stay intact. |
| "The insert worked, done" | Check the errors of every later feature. |
| "Edge points with expressions, so the chamfer follows the size" | They are rejected, and stored picks already follow size changes. |
| "Pick all 8 rim edges" | One pick on a tangent chain fillets or chamfers the whole chain. |
| "Measure after every edit" | One batch, one `measure` against the summed prediction. Split only when it misses. |
