# CAM: what posting a real part found in the first slice

The first CAM slice (`solidcraft-cam`, the `cam.*` commands) was used to post a machined aluminium
housing (130 × 100 × 36 mm: a cavity, an O-ring groove, twelve holes, an outline with mounting
ears) for a Synthetos TinyG controller. The programs were checked with the firmware's own parser
and planner on the host, never on a machine. Everything below is a defect or a gap in the slice, not
in any controller, and each is written here so the fix can be reviewed against the evidence. The
G-code writer's fixes from this list are in the same change set as the `tinyg` post; the rest are
open.

| # | What happened | Where | State |
|---|---|---|---|
| 1 | The first rapid after a tool change moved XY before Z. The writer forgets the tool's position at the start of every operation and went across first when the position was unknown, so after `M0` the tool travelled sideways at whatever height the operator left it. Both parsers and read-back accept either order; it was seen by reading the posted program's order after a planner replay, not flagged by any check. | `gcode.rs` `rapid()` | Fixed: an unknown position rises first. |
| 2 | Coolant across an attended tool change. `M0` does not turn coolant off, and the writer only re-emits `M8` when an operation's coolant flag changes, so a post template cannot write `M9` at the change without leaving the next operation dry. | `gcode.rs` `emit()` | Open: treat a tool change as a transition (retract, spindle off, coolant off, pause, restore). |
| 3 | A 3° ramp into a loop far smaller than the tool. A 5.5 mm hole with a 3 mm end mill, drawn as a polygon, offset down to a 0.04 mm ring and ramped into it: 17,000 moves of 0.005 mm for one hole. | `ops.rs` `pocket()` | Fixed in part: a ring is cut only while the previous one leaves material inside its reach, and a region the tool does not fit is refused, identified by its number. Ramps into loops shorter than the ramp still want a plunge or a helix. |
| 4 | Read-back took 7 s for 17k lines and about 30 minutes for 560k. `directed()` already buckets segments in a grid; the pathology is elsewhere (very many near-coincident micro-segments in one cell, or the sampling) and wants a profile on that program. | `readback.rs` | Open: profile, then fix the hot path or compare per operation. |
| 5 | A through contour has no tabs or skin, so whether the part stays put depends entirely on the workholding; the housing was cut 0.5 mm short of the bottom by hand. | `ops.rs` `contour()` | Open: `tabs` or `skin` on the contour. |
| 6 | No drilling or helical bore operation; a tap drill the size of the tool cannot be pocketed, and a side port needs a second setup. | operations | Open. |
| 7 | Depth passes retract above the stock and plunge back at the plunge feed through material already removed, and several regions in one operation are cut in turn on every pass. The housing's four 36 mm holes spent 30 of 43 minutes plunging through air. | `ops.rs` linking | Open: rapid to just above the previous floor, finish a region before the next. |
| 8 | Nothing between the operation and the file knows what the controller can use. The same housing with holes as polygons had 436 runs of 48 moves under 0.01 mm, more blocks per millimetre than the controller's planner and the sender can keep up with; with the holes as sketch circles it had none. Both passed every check, and both are geometrically valid programs. | writer, posts | In part: the pocket no longer makes rings inside the tool's reach; a post's `min_move` lets `cam.post` report how many straight moves are shorter than the controller plans well, changing nothing. Polylines still never become arcs, which the operation's parameters do not say. |

## What the checks do and do not prove

- `verify()` refuses non-finite values, cuts below the bottom, some rapid and entry patterns and bad arc radii (the heights' order is checked when an operation is set up, not here). It has no model of the stock, the fixture or the tool's envelope, so it cannot refuse a gouge or a rapid through material in general.
- Read-back parses the written program and holds its cutting moves against the planned ones.
- A controller's parser (TinyG's, run on the host) refuses words it does not know.
- A planner replay (TinyG's, run on the host) finds blocks its planner refuses or alarms on, and lets a person read the planned order; it does not detect unsafe orderings by itself.

None of these knows the stock, the fixture or the real tool, and none replaces looking at the
toolpath. A program with many moves shorter than the controller plans well is not wrong, but it
will run slower than its feed says and may stall; the operation that made it is the place to ask why.
