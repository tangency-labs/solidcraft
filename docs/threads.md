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

| Parallel pipe | `bspp` | `G 1/2` | 27.5° / 27.5° | h = 0.640327 P of H = 0.960491 P | (H − h) tan 27.5° = 0.1667 P, both | d − 2h | d − 2h, d |
| Parallel internal (ISO 7-1) | `rp` | `Rp 1/2` | 27.5° / 27.5° | h = 0.640327 P of H = 0.960491 P | (H − h) tan 27.5° = 0.1667 P, both | (internal only) | d − 2h, d |
| Taper pipe (ISO 7-1) | `bspt` | `R 1/2`, `Rc 1/2` | 27.5° / 27.5° | h = 0.640327 P of H = 0.960237 P | (H − h) tan 27.5°, both | d − 2h at the gauge plane | d − 2h, d at the gauge plane |
| NPT | `npt` | `1/2-14 NPT` | 30° / 30° | h = 0.8 P of H = 0.866025 P | (H − h) tan 30° = 0.038 P, both | E − h at the plane of E | E − h, E + h |

The pipe threads' crest and root rounds (ISO 228-1/7-1 r = 0.137329 P, 0.137278 P taper) are
modelled as flats at the basic height, as the other forms' radii; NPT takes the minimum truncation
0.033 P at both. The flanks' bisector is square to the axis in all three.

**Taper pipe threads** (`bspt`, `npt`): the diameters change 1 in 16 along the axis. The thread
starts at a free end, the pipe's end or the hole's mouth (`from`), with the diameter its standard
puts there: an external thread its major diameter at the gauge (reference) plane its gauge length
in from the small end (ISO 7-1: the basic gauge length, R 1/2 20.955 mm at 8.2 mm; NPT: E1 + h at
the hand-tight length L1, E1 = E0 + L1/16), an internal one the reference diameter at its face
(ISO 7-1's gauge plane; NPT's E1 at the large end). The groove is SMLib's screw sweep with its
section moved 1/32 of the axial travel off the axis per unit length (the radial law that also
makes runouts), its walls within the sweep's tolerance of the true surfaces; its section reaches
just past the crest, and the material beyond the major cone (external) or inside the minor cone
(internal) is cut with it, as the die or tap cuts the crests. Past the major cone the crests
vanish into the pipe's surface (ISO 7-1's incomplete thread). An external taper thread runs its
standard length unless `length` or `up_to` says otherwise (ISO 7-1's useful thread for the basic
gauge length, NPT's effective thread L2); an internal one the face. A pipe thread has one start and
turns one way. A taper thread's chamfered tip (`tip`, with its `chamfer_depth`) is measured from the
major diameter the taper has at that end. ISO 228-1's G sizes are ISO 7-1's gauge-plane diameters
(1/16 to 6); ISO 228-1's extra sizes (5/8, 7/8, …) are not tabulated yet. ISO 7-1's parallel
internal thread Rp (`rp`, `Rp 1/2`), the one an R screws into, has the parallel form (H = 0.960491
P) at the gauge diameters; it is internal only.

Trapezoidal clearance ac (ISO 2904): 0.15 mm for P = 1.5, 0.25 mm for P 2 to 5, 0.5 mm for P 6 to
12, 1 mm for P 14 to 44.

The square thread is refused: no current ISO, ASME or DIN standard defines one. Machinery's
Handbook gives only historical proportions (depth and width P/2; the "modified square" 10°); the
trapezoidal and Acme forms replaced it.

**Sizes.** Without a designation the thread takes the standard size that fits the face: a shaft
its major diameter, a hole its drill (metric: the tap maker's drill, `docs/thread-catalogue.md`;
the other forms: the internal minor diameter). Metric uses the ISO 261/262 coarse pitches (M1.6 to M56); trapezoidal and metric
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

**Runout** (`ease_out`: a length, `standard` = 2.5 P or `short` = 1.25 P, ISO 3508's normal and
short thread runouts x): the thread's inner end, away from where it starts, fades out over that
length, shorter than a turn too: its section moves out of the material as it turns (an external
thread's away from the axis, past the surface; a nut's towards it, into the hole), linearly, by
SMLib's radial screw sweep, and no end face is left. Without it the groove stops square there,
ending in its own profile, a flat face in the axial plane. The runout lies past the full-form
length: beyond a square stop, and before a shoulder up to its clearance (the full form then ends a
runout short of it); at a free end it lies inside the face, the groove reaching nothing at the
end. The start never fades: it runs off its free end, cut by the end face, plain or chamfered
(`tip`); `ease_in` is refused. Where the section moves, the sweep's walls are within its tolerance
(1e-4 P) of the true surfaces; full-depth turns are exact radially.

**Tip** (ISO 4753's chamfered end; `tip: start` the end the thread starts from, which must be
free, `tip: both` each free end it reaches): at those ends of a modelled external thread, after the groove, the material outside a cone `chamfer_angle` to the axis (45°) reaching
d_f = d − 2 `chamfer_depth` at the end is cut, the last teeth tapering into it. ISO 4753 bounds
d_f by the minor diameter; the default is that bound, the external minor diameter d3 = d −
1.226869 P of ISO 724 (8.160 for M10), which lies below the modelled basic root d1. The cone
therefore crosses the root and leaves the end a whole disk of d3 inside a whole conical ring; a
cone ending on the root itself would pinch to nothing wherever the groove reaches the end. ISO
4753 is for ISO metric threads, so the other forms give `chamfer_depth`. An internal thread takes
the hole's countersink instead.

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
  the chamfered tip. ISO 4753:2011, figure 1 b) (chamfered end CH; the public preview at
  <https://cdn.standards.iteh.ai/samples/55928/38ddde12863b4c2db35dc929fd502a4d/ISO-4753-2011.pdf>):
  the end's diameter "Maximum diameter at the minor thread diameter", the angle ≤ 50°, the
  incomplete thread u ≤ 2 P.
- ISO 7-1:1978 (taper and parallel 55° profiles: H = 0.960237 P / 0.960491 P, h = 0.640327 P,
  r = 0.137278 P / 0.137329 P, figures 1 and 2; Table 1, columns 1 to 8 and 15: threads per 25.4
  mm, major diameter at the gauge plane, basic gauge length, useful thread), from the public
  preview <https://cdn.standards.iteh.ai/samples/2856/96e1948d3d734f28b2563bcab2920245/ISO-7-1-1978.pdf>;
  the same diameters as G sizes in <https://en.wikipedia.org/wiki/British_Standard_Pipe> and the
  Yamawa YEU23 tables `pipe_thread_catalog.rs` cites.
- ASME B1.20.1 (NPT) as NBS Handbook H28 (1957) Part II, section VII publishes it: 60°, H =
  0.866025 p, h = 0.800 p, truncation at least 0.033 p, taper 1 in 16, E0 = D − (0.05 D + 1.1) p,
  E1 = E0 + 0.0625 L1, L2 = (0.80 D + 6.8) p, and Table VII.2 (D, n, E0, L1, L2 for 1/16 to 12):
  <https://nvlpubs.nist.gov/nistpubs/Legacy/hb/nbshandbook28supp1957pt2.pdf>.
- ISO 724 (external minor diameter d3 = d1 − H/6 = d − 1.226869 P, M10x1.5: 8.16), as given at
  <https://www.valvias.com/miscellanea-metric-thread-din-13.php>.
