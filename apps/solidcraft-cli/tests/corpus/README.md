# Design file corpus

Designs saved by older SolidCraft builds. `tests/design_corpus.rs` opens each one with the
current build and checks that it is upgraded to the current file format, that it evaluates
(a feature may fail only by asking for a reference it lost to be picked again; what it rebuilds
to is today's), and that damaged and hostile variants of it never panic. The `.expected.json`
next to each is `solidcraft-cli eval` of that file by the build that saved it (bodies, volumes,
areas, world bounding boxes), kept as a record of what that build made; no test compares with
it.

| File | Saved by | Era |
|---|---|---|
| `m0-box-fillet-hole` | 3714342 | M0: sketch, parameters, extrude, fillet, cut |
| `m0-revolve-primitives` | 3714342 | M0: revolve, primitives, combine, chamfer, timeline edit |
| `components-arm` | 97d03e3 | components and occurrences: move, copy, Paste New, ground |
| `joints-hinge` | c2309eb | joints: revolute hinge driven by a parameter |
| `modern-sheet-metal` | bd98035 | sheet metal contour flange (oracle 72) |
| `modern-path-pattern` | bd98035 | pattern on path (oracle 53) |
| `modern-plastic` | bd98035 | shell, lip, boss, rest, snap fit, plastic rule |
| `naming-split-pieces` | 9a979cf | format 13 names: a slot splits the top; fillet, chamfer and offset face name its pieces by number (`F2:top#2`) |
| `move-bodies` | 00baf1d | `solid.move` before it could copy: two bodies, one translated in place, one translated and turned 90 deg about Z (no `copy` key anywhere in the file) |
| `split-body-pieces` | v0.3.2 (07e263d) | a slot along X cuts a block into two bodies; a fillet, a chamfer and an appearance refer to the pieces by body name (`Body1`, `Body1 (1)`) |
| `issue-489-stacked-join` | v0.6.0 (809ecf7), desktop | a 70 × 84 extrude, then a 70 × 62 strip joined on its top face, sketched partly over the face's projected edges (issue #489; expected values from main at 2880525) |
| `issue-616-lid-ribs` | v0.7.0 (a9879fa), CLI | a rounded lid with a skirt ring, a window and a slot cut through, then four ribs joined on the plate over the skirt, flush with the window's sides; v0.7.0 displaced the ribs and left a slit through the skirt and plate (issue #616; script `examples/regressions/lid_ribs_flush_with_window.json`, expected values from the fix) |

The scripts that made them are in `scripts/` (`issue-489-stacked-join` was drawn in the desktop
app and has none; `issue-616-lid-ribs` is made by its regression script). Every flat face of every final body must also take an attached sketch and an
offset plane; the faces that can't yet are listed with their issues in `design_corpus.rs`.

Never regenerate a file with a newer build: the point is that it was written by the old one. Add new files for new eras instead.
