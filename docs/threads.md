# Threads

`solid.thread` puts a thread on a cylindrical face: cosmetic (an annotation, `model.threads`
lists it) or modelled (the groove between the teeth cut into the body along a helix, SMLib's exact
screw sweep). This page gives the forms, the values each uses and where they come from, and how a
thread's extent is decided. The code is `crates/doc/src/thread_forms.rs` (forms, sizes,
designations) and `crates/doc/src/eval_thread.rs` (the thread on its face, the cut).

## Forms

Every groove is a polygon in an axial half-plane: straight flanks, flat crest and root. Crest and
root radii (the ISO metric root radius, the buttress root radius R, DIN 405 knuckle and Whitworth
rounds) are not modelled. External threads are cut from the shaft's surface (a little past it, so
no skin is left) down to the external thread's minor diameter; internal threads from the hole
wall out to the internal thread's root. Pitch-diameter allowances and tolerance classes are not
modelled: a modelled thread is the maximum-material design profile below.

| Form | `form` | Designation | Flanks | Depth | Crest flat | External minor | Internal minor, root |
|---|---|---|---|---|---|---|---|
| ISO metric | `metric` | `M10`, `M10x1`, `M16xPh3P1.5` | 30° / 30° | 5H/8, H = P √3/2 | P/8 at d, P/4 at D1 | d − 5H/4 | D1 = d − 5H/4, D = d |
| Trapezoidal | `trapezoidal` | `Tr20x4`, `Tr40x14(P7)` | 15° / 15° | H1 = P/2 | P/2 (1 − tan 15°) = 0.366 P | d3 = d − 2 (H1 + ac) | D1 = d − P, D4 = d + 2 ac |
| Acme (general purpose) | `acme` | `1/2-10 ACME` | 14.5° / 14.5° | h = P/2 | P/2 (1 − tan 14.5°) = 0.3707 P | d − P − 0.020″ (0.010″ finer than 10 tpi) | D − P, D + 0.020″ (0.010″) |
| Metric buttress | `buttress` | `S20x4`, `S40x14(P7)` | 3° (load) / 30° | H1 = 0.75 P | (P − 0.75 P (tan 3° + tan 30°))/2 = 0.26384 P | d3 = d − 2 × 0.86777 P | D1 = d − 1.5 P, D = d |
| Buttress inch | `buttress_asme` | `2.5-8 BUTT` | 7° (load) / 45° | h = 0.6 P | (P − 0.6 P (tan 7° + 1))/2 = 0.16316 P | d − 1.2 P (basic) | d − 1.2 P, d (basic) |

Trapezoidal clearance ac (ISO 2904): 0.15 mm for P = 1.5, 0.25 mm for P 2 to 5, 0.5 mm for P 6 to
12, 1 mm for P 14 to 44.

The square thread is refused: no current ISO, ASME or DIN standard defines one. Machinery's
Handbook gives only historical proportions (depth and width P/2; the "modified square" 10°); the
trapezoidal and Acme forms replaced it.

**Sizes.** Without a designation the thread takes the standard size that fits the face: a shaft
its major diameter, a hole its drill (metric: the tap drill d − P; the other forms: the internal
minor diameter). Metric uses the ISO 261/262 coarse pitches (M1.6 to M56); trapezoidal and metric
buttress the ISO 2902 middle pitch of each diameter (Tr8x1.5 to Tr130x14; buttress from S10x2),
the series DIN 513-2 shares; Acme ASME B1.5's preferred sizes (1/4-16 to 5-2). Buttress inch has
no size table here: its pitch is always given.

**Hand.** `hand: right | left | both`. A left-handed groove is the right-handed one's mirror image
(the screw motion about the reversed axis, rising along it); `both` cuts a right- and a
left-handed groove crossing on the face (a reversing screw).

**Buttress direction.** `steep_flank: end | start` says which way the steep (load) flank of the
screw's teeth faces along the thread's axis; a nut cut with the same setting mates with it (its
own steep flanks face the other way).

## Extent

`extent: full | length | up_to`, `length`, `up_to` (a point's place along the axis, or where a
plane crosses it), `from` (a point near the end the thread starts at).

A face end is **free** when no material lies past it in the groove's way: over the span from one
pitch to one and a half past the end, within the radii the groove cuts (the external minor
diameter to the major, a nut's minor diameter to its root, and the face itself). A cap, a chamfer
the groove runs out through, a counterbore wider than the thread are free; a shoulder, a head
whose corners reach past the root, a blind hole's bottom are not.

- At a free end the groove runs out a pitch past the end, into the air.
- At a closed end it stops with its section clear of the end: its centre as far inside as the
  section reaches that way, so it never cuts into the shoulder.
- Inside the face it stops square. For a symmetric section a square stop inside material removes
  exactly length / P turns (what the last section leaves uncut ahead of the stop, it cuts behind
  it in mirror image).

A thread of some length starts at the end nearest `from`; without it, at the face's only free end;
without one, at the end the `face` point lies within a third of the face of. A pick in the middle
of a face whose ends are alike is refused, naming `from`. A length as long as the face runs the
whole face.

## Runout and tip

**Runout** (`ease_in`, `ease_out`: a length, `standard` = 2.5 P or `short` = 1.25 P, ISO 3508's
normal and short thread runouts x): the groove fades out over that length, its section moving
out of the material as it turns (an external thread's away from the axis, past the surface; a
nut's towards it, into the hole), linearly, by SMLib's radial screw sweep. `ease_in` is at the end
the thread starts from (`from`, or the start of its extent), `ease_out` at the other. The runout
lies past the full-form length: beyond a square stop, and before a shoulder up to its clearance
(the full form then ends a runout short of it); at a free end it lies inside the face, the groove
reaching nothing at the end. No end gets a runout unless asked. Where the section moves, the
sweep's walls are within its tolerance (1e-4 P) of the true surfaces; full-depth turns are exact
radially.

**Tip** (`tip: chamfer`, ISO 4753's chamfered end): at each free end of a modelled external
thread, after the groove, the material outside a cone `chamfer_angle` to the axis (45°) reaching
d_f = d − 2 `chamfer_depth` at the end (default the external minor diameter, the largest the
standard allows) is cut, the last teeth tapering into it. An internal thread takes the hole's
countersink instead.

## Sources

- ISO 68-1 (basic profile), ISO 261/262 (sizes), ISO 965-1 (multi-start designation `Ph`).
- ISO 2901 to 2904 (trapezoidal: basic and design profiles, series, basic dimensions); the
  design diameters checked against the Bornemann Gewindetechnik tables of DIN 103
  (Tr10x2: d3 7.5, D4 10.5, D1 8; Tr40x7: 32, 41, 33; Tr100x20: 78, 102, 80):
  <https://www.bornemann-gewindetechnik.de/de/produkte/trapezgewinde/daten/>.
- DIN 513-1/-2 (metric buttress): the formulas (ac = 0.11777 P, h3 = 0.86777 P, flat w =
  0.26384 P, D1 = d − 2 H1) and the diameter-pitch table as IS 4696 (Part 3): 2004 publishes them
  in line with DIN 513 (<https://law.resource.org/pub/in/bis/S01/is.4696.3.2004.pdf>), and the
  Bornemann tables (S10x2: d3 6.529, D1 7; S40x7: 27.851, 29.5):
  <https://www.bornemann-gewindetechnik.de/de/produkte/saegengewinde/daten/>.
- ASME B1.5 (Acme): flat 0.3707 P, h = P/2, the 0.020″/0.010″ minor and major allowances and the
  preferred sizes, as reproduced at <https://torqbolt.com/unified-acme-threads-dimensions-specifications>.
- ASME B1.9 (buttress inch): 7°/45°, basic height of engagement 0.6 P; the flat follows from the
  fundamental triangle H = P / (tan 7° + tan 45°) = 0.89064 P truncated equally at crest and root.
- ISO 3508 (thread runouts) and ISO 4753 (ends of parts with external threads) for the runout and
  the chamfered tip.
