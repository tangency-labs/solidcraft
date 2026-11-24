# Shaft splines

Shaft Spline (#17693) cuts a spline into a round shaft or a bore, or makes a new splined shaft and
the hub that fits it. It is one feature in the History (`shaft_spline.insert`): reopen it to change
the teeth, the sizes or the fit. This page covers what it does and how its numbers work. The code
is `crates/doc/src/shaft_spline.rs` (the checks and the plan), `crates/doc/src/eval_shaft_spline.rs`
(the rebuild) and `crates/ui-egui/src/dialogs_shaft_spline.rs` (the dialog).

## The workflow

1. **Open Shaft Spline** (MODEL › CREATE, in the Create menu). Pick the outside of a round shaft, the wall of a
   bore, both, or nothing. For each round face picked, pick the face its teeth start from next.
2. **Choose the teeth**: the **Type** (Involute, Straight-sided or Serration), the **Teeth** count
   and the **Length**. The drawing at the top of the dialog follows every change, and the line under
   it says in words when the numbers make no spline.
3. **Set the fit**, the chamfers, and a master spline if the hub must go on one way only. These are
   in groups of the dialog that open and close (see [The dialog](#the-dialog)).
4. **Accept.** The History gets one Shaft Spline feature.

```json
{"command": "shaft_spline.insert", "params": {"type": "involute", "make": "both", "teeth": 16, "module": 2, "length": 30, "side_clearance": 0.05, "tip_clearance": 0.1, "hub_diameter": 60}}
{"command": "shaft_spline.insert", "params": {"shaft_face": [0, 6, 20], "shaft_start": [0, 3, 40], "type": "straight", "teeth": 6, "length": 25, "major_diameter": 12, "minor_diameter": 10, "tooth_width": 2, "side_clearance": 0.05, "major_clearance": 0.1, "minor_clearance": 0.1}}
```

## What is picked decides what is made

- **Nothing picked.** A new part stands on the origin along +Z, from z = 0 to the length. **Make**
  is Shaft, Hub or Shaft and hub. A new hub needs its **Outside diameter** (`hub_diameter`), which
  must leave a rim around the hub's spaces.
- **The outside of a round shaft** (`shaft_face`) is cut with the spline's tooth spaces.
- **A bore** (`bore_face`) is cut with the hub's teeth.
- **Both** cut a shaft and a hub that fit each other.

With a face picked, **Make** is not offered: what is cut follows the picks.

## Where the teeth start

A round face has two ends, so each picked part takes a second pick: **Shaft start** (`shaft_start`)
and **Bore start** (`bore_start`), a point on the face the teeth start from. That face is the part's
flat end, a shoulder, a step, a chamfer or a round. The teeth start at the end of the round face
nearest that face along the axis, and run into the round face for the **Length**. A chamfer added
later between the two does not change the end.

The dialog needs the start face of every part it cuts: OK waits for it, and the line under the
drawing says which face is missing. A script may leave `shaft_start` or `bore_start` out. The
command then takes the end of the round face nearest `shaft_face` or `bore_face` and stores a point
at that end as the start, so the spline keeps its end when the part is made longer or shorter
later. `shaft_start` is refused without `shaft_face`, and `bore_start` without `bore_face`.

The **Length** is measured along the picked round face from the start end, and a length longer than
the face is refused. Angle 0 for a picked face comes from the world axes: the axis the face's own
axis is closest to decides, and its reference direction (+X for Z, +Y for X, +Z for Y) is projected
onto the section.

## The teeth

| Type | `type` | Teeth | Numbers |
|---|---|---|---|
| Involute | `involute` | 3 to 2000 | `module`, `pressure_angle` (default 30°), `addendum`, `dedendum`, `root_fillet`, `profile_shift`, `thinning` |
| Straight-sided | `straight` | 2 to 2000 | `major_diameter`, `minor_diameter`, `tooth_width`, `root_round`, `tip_corner` with `tip_size` |
| Serration | `serration` | 2 to 2000 | `major_diameter`, `minor_diameter`, `flank_angle` (default 90°), `root_round`, `tip_corner` with `tip_size` |

Every number is a number or an expression, so it can be a parameter. The tooth count is a whole
number. `tip_corner` is `plain`, `chamfered` or `rounded`.

**Fit.** Involute teeth take `side_clearance` (both sides together, as an arc on the pitch circle),
`hub_addendum`, `tip_clearance`, `form_clearance` and `hub_root_round`. Straight-sided teeth and
serrations take `side_clearance` (each side, square to the flanks), `major_clearance`,
`minor_clearance`, `hub_root_round` and `hub_tip_round`.

**As made.** The dialog's read-outs give what the numbers come to: the positions the hub fits in,
the turning play in degrees, the play on each side, the gap at the tips and at the roots, and, for
involute teeth, the gap at the fillet.

## The shaft's diameter

By default the shaft's diameter follows the spline's numbers: more teeth of one size make a larger
shaft, and a picked shaft is built up or turned down to the spline's major diameter before the
spaces are cut. With **Fixed diameter** ticked (`shaft_face` with the size left out) the spline keeps
the picked shaft's own diameter and works its size out from it: for involute teeth the module
follows the tooth count, for the other types the major diameter is the shaft's.

## Master spline

`master_spline` is `none`, `tooth_out` (a tooth left out) or `wide_tooth` (a space filled), so the
hub goes on in one position only. `master_angle` says where the middle of the master spline sits,
in degrees counter-clockwise from angle 0 (the X axis for a part along Z); the whole spline turns to
put it exactly there. The drawing's **Show** choice then also offers the master spline.

## Groove end

On a picked shaft, **Round the ends** (`cutter_diameter`) makes each tooth space end in the curve a
cutter of that diameter leaves. That curve starts where the length ends and runs on until it leaves
the shaft's surface, through a shoulder if one stands there, as a cutter would. Without it the
spaces end square. `cutter_diameter` is refused without `shaft_face`. Teeth that run the whole length
of the face run right through, with no rounded end.

## End chamfer and entry chamfer

`end_chamfer` is a chamfer on the ends of a shaft's teeth: it runs `end_chamfer` along the shaft and
in from the major diameter. `entry_chamfer` is a chamfer where the teeth run out of a hub's bore: it
runs `entry_chamfer` into the bore and out from the tops of the hub's teeth. Each takes the Chamfer
tool's three types, which set how far it runs across the end:

| Type in the dialog | Parameters | Across the end |
|---|---|---|
| Equal Distance | `end_chamfer` alone | as far as along: 45° |
| Two Distances | `end_chamfer` with `end_chamfer_distance2` | `end_chamfer_distance2` |
| Distance and Angle | `end_chamfer` with `end_chamfer_angle` | `end_chamfer` × tan(`end_chamfer_angle`) |

The same for the hub with `entry_chamfer_distance2` and `entry_chamfer_angle`. The angle is measured
from the round face, in degrees, above 0 and below 90; the second distance is more than 0. A second
distance or an angle is refused without its chamfer, and the two are refused together for one
chamfer. A chamfer of 0 is none, whatever else is given.

```json
{"command": "shaft_spline.insert", "params": {"type": "straight", "make": "both", "teeth": 6, "length": 20, "major_diameter": 28, "minor_diameter": 23, "tooth_width": 6, "side_clearance": 0.05, "major_clearance": 0.3, "minor_clearance": 0.2, "hub_diameter": 40, "end_chamfer": 1, "end_chamfer_angle": 30, "entry_chamfer": 1, "entry_chamfer_distance2": 0.5}}
```

Both chamfers are cut by the spline itself, so they follow the size and the tooth count when these
change. On a picked face each is cut on the end the teeth start from, and on the far end too where
the teeth run the whole face; on a new part, on both ends. A part that was chamfered or rounded first
with Chamfer or Fillet is cut through, and the teeth run out through that chamfer or round. 0 means
none. `end_chamfer` and its second distance and angle are refused when no shaft is cut or made,
`entry_chamfer` and its own when no hub is.

## The dialog

The numbers that size the teeth stay at the top: the fit check box (a picked shaft), the drawing,
**Type**, **Make** (nothing picked), **Teeth**, **Length**, then **Module** and **Pressure angle**
(involute), or **Major diameter**, **Minor diameter** and **Tooth width** or **Flank angle**. The rest
is in groups that open and close by their titles, in this order: **Tooth form** (the involute's
addendum, dedendum, root fillet, profile shift and thinning, or the root round; open), **Fit**
(open), **Tip corners** (straight-sided teeth and serrations; closed), **Chamfers** (when a shaft or
a hub is cut or made; closed): each chamfer's size, its **Type** and the **Distance 2** or **Angle**
its type needs, **Groove end** (a picked shaft; closed), **Master spline** (closed), **Hub** and
**As made**. The dialog sends a chamfer's second distance or angle only with the chamfer, and only
for the type that uses it.
