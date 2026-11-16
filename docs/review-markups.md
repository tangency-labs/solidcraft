# Review markups

Mark what needs changing on a design, leave a note and pictures, and hand the set to a person or an agent: "open the design, read the markups and do as they say". The one who works through them changes the design with the usual commands, then closes each markup as done or not done, with a reason. A markup never enters the design: it names a face, an edge, a body, a feature or a component and owns none of them, so writing markups changes neither the design file nor its undo history.

## The set

- Beside the design: `bracket.sc` has `bracket.sc.markups.json`, and its pictures and lock in `bracket.sc.markups/`. Every command takes `set` to use another file (its name ends with `.markups.json`); a design with no file yet needs one.
- The file is plain JSON, written without indents, readable without the app: `format`, `next_id` and `markups`. A markup has an `id` (never used again) and a `guid`, a `status` (`open`, `resolved`, `rejected`), its `thread`, its `anchor`, the `view` it was marked from (the orbit camera's target, yaw, pitch, distance and field of view), its `images`, `by` (who made it: the reviewer), `made` and `changed` (Unix seconds), and while closed, its `outcome` (`by`, `reason`, `at`).
- The thread is what was said, oldest first: the note is its first message, then each reply. Closing with a reason puts the reason in the thread too. A message from anyone but the markup's maker is at most 280 characters; a longer one is refused whole, never cut, with an error that says the limit, its length and how far over it is. The maker writes up to 4,000. Only the maker changes the note; anyone may reply, pin it again, close it or reopen it.
- Every write is one step under a lock on `<set folder>/lock`: read the file as it stands, change it, check the whole set, write it whole (beside it, flushed, then renamed into its place). Two writers, the window and an agent, never lose each other's changes, and a killed writer leaves the last whole file. A picture's file is on the disk before the set names it, and comes off again if the set refuses it.
- `format` is 1. A set with a higher number, made by a newer SolidCraft, is refused with a message and left as it is. A key the format does not know is refused too, since a write puts the set back whole and would drop it.
- Limits: 2,000 markups, 200 messages a thread, 8 pictures a markup, 16 MiB a picture, 1 GiB of pictures a set. A picture is a PNG or a JPEG by its first bytes, kept as `<guid>.png` or `.jpg`; a picture's file name in a set is checked, so a set can't point out of its folder.
- In the browser there is no file beside the design: the set is kept where the web app keeps its files, in the browser's own storage, and there is one writer, so no lock.

## What a markup is on

`markup.add` takes one of these; with none, its `view`; with no `view` either, the one thing selected.

| Given | Anchor | Kept |
|---|---|---|
| `face` (+ `body`, `at`) | the face | the body, the feature that made it, the face's lasting name (as `model.faces` lists it), the point marked, the active configuration |
| `edge` (+ `body`, `at`) | the edge | the same, with the edge's name (as `model.edges` lists it) |
| `body` + `at` | the face of the body the point lies on | as `face` |
| `body` | the body | the body, the feature that made it |
| `at` | the face of any body the point lies on, else the point | as `face`, or the point |
| `feature` | a feature of the timeline | its id and name |
| `occurrence` | a placed component | its occurrence path and name |
| `view` | the view itself | |

Which face a point lies on is decided with exact geometry, the B-rep's own faces, never the display mesh; so is where a face is now.

## Where it stands now

`markup.list` says of each anchor where it stands in the model now (`found`):

- `anchored`: it is there, and the point marked still lies on it.
- `moved`: it is there, found by its lasting name, but no longer where it was marked; `at` is the nearest point of it now, `was` where it was and `moved_by` how far apart they are. A face made taller by a rebuild keeps its markup.
- `hidden`: it is not in the model now, but the features that make it are still in the design: the history marker is before one of them, one is suppressed, or another configuration is active than the one it was marked in. `why` says which. Rolling forward, unsuppressing or switching back shows it again.
- `gone`: what made it was deleted or replaced; `why` says what it was, in words that name no id.
- `unchecked`: it could not be measured.

A body renamed since is found again by the feature that made it.

## Commands

| Command | Does |
|---|---|
| `markup.list` | the set, each markup with its thread, anchor, `found`, view, pictures and outcome; `status` keeps one status |
| `markup.add` | a new open markup with its note (`text`), on what the call names |
| `markup.edit` | changes the note (its maker only), pins it again, or changes its view |
| `markup.reply` | adds a message to the thread |
| `markup.image` | attaches a picture (`file`, or `data` in Base64, as a window's screenshot comes) or takes one off (`remove`) |
| `markup.resolve`, `markup.reject` | close an open markup as done or not done, with a `reason` |
| `markup.reopen` | opens a closed one again, with an optional `reason` |
| `markup.delete` | takes a markup and its pictures away |

A markup is named by its number or its guid (`markup`). `by` says who writes; without it, `agent`. Each has input and output schemas (`engine.describe`). They need no window and open no dialog, so they run the same from a script on the design (`solidcraft-cli run review.json --in bracket.sc`), MCP and the control channel:

```json
{"commands": [
  {"command": "markup.list", "params": {"status": "open"}},
  {"command": "markup.resolve", "params": {"markup": 3, "reason": "Wall now 4 mm"}}
]}
```

## Tests

`crates/io/src/markups_tests.rs` tests the file: a frozen format 1 set, a newer one refused and kept, unknown keys refused, the one check, two writers at once and pictures. `crates/engine/src/cmd/markups_tests.rs` tests the commands: a round trip that leaves the design and its undo history alone, anchors from a call, the selection and the view, a rebuild that keeps a face's markup and says where it moved, a face hidden by the history marker, a suppression and a configuration and gone once deleted, the reply limit, pictures, two sessions writing at once, and hostile values in every key.
