---
name: solidcraft-printing
description: Use whenever a SolidCraft design will be 3D printed, before and while modelling it — wall thickness, fits and clearances, screw bosses, heat-set inserts and nut traps, overhangs and support-free design, print orientation and layer strength, shrinking holes, elephant foot, splitting a part too big for the bed, assembly order, and the checks to run before calling a design printable. To draw the geometry itself, use solidcraft-modeling.
---

# SolidCraft printing

## Overview
A part that looks right in the viewport can still be unprintable, unweldable or unassemblable. The failures are not geometric errors, so SolidCraft reports nothing: a 0.8 mm wall prints as a gap, a ⌀3 hole arrives at ⌀2.8, an M3 screw splits the boss it goes into, a flat roof needs 60 mm of support, and two parts that must slide are modelled touching. Every rule below is a number you can check in the design before export.

Design for **FDM** with a **0.4 mm nozzle** unless told otherwise. Ask for the printer's build volume and its measured hole shrink if the user has them; if not, state the values you assumed in your reply, because they change the fits.

`$SKILL` is the folder this file is in. Written against SolidCraft 0.12.

## Before the first call
1. Ask, or write down and state: **build volume** (X × Y × Z), **nozzle**, **material** (PLA, PETG, ASA/ABS), **layer height**. Everything below scales from these.
2. **Fit stack.** Decide what must slide, what must press, and what is screwed. Put each clearance in a named parameter (`fit_slide`, `fit_close`, `fit_press`) so the user can retune for their printer in one place instead of hunting through sketches.
3. **Split plan.** Nothing may exceed the build volume. Decide where it splits before modelling, not after: a split through a smooth face is invisible, a split through a screw boss is not.
4. **Orientation.** For each printed part decide which face is on the bed. That decides what overhangs and where the layers run, and it is cheaper to choose now than to redesign later.

## Walls and features
- **Structural walls 2.0 mm or more; never 0.8–1.2 mm.** Below three extrusion widths (3 × 0.45 = 1.35 mm) the slicer prints a thin wall with gaps or a single bead that snaps. A 0.8 mm wall is weaker than 3 perimeters and looks worse.
- **Wall thickness does not need to be a multiple of the extrusion width**, but 1.6 / 2.0 / 2.4 mm print evenly with a 0.4 mm nozzle and 2.0 is the default for anything under load.
- **Thick solid blocks waste material and warp.** A 10 mm block becomes a shell with infill; use 2.5–3 mm walls and ribs instead. Rounded, ribbed corners beat square solid corners for both strength and cooling.
- **Inside corners are stress risers.** Give a fillet of at least 1 mm, ideally 0.25 × the wall, anywhere a wall meets a wall or a boss.
- **Minimum detail**: 0.8 mm wide, 0.6 mm proud for embossed text; 4 mm cap height minimum for legible embossing; engraved text needs 0.4 mm depth or it fills in.
- **Keep the bottom face flat and continuous.** A bottom with pockets and holes loses bed adhesion. Chamfer the first 0.6 mm at 45° to hide elephant foot (~0.3 mm of bulge), rather than trying to compensate the other way.

## Fits, holes and fasteners

| What | Design value | Notes |
|---|---|---|
| Sliding fit | 0.3 mm per side (0.6 total) | A drawer, a lid lip, anything that moves after assembly |
| Close fit, hand-assembled | 0.15–0.2 mm per side | Aligned parts, drop-in inserts, not meant to move |
| Press fit | 0.0 to −0.05 mm | Only on small parts that can be tapped in; plastic yields, so it loosens with reuse |
| Print-in-place gap | 0.4 mm | Below 0.3 mm the layers weld together |
| Vertical hole shrink | subtract 0.1–0.4 mm from the modelled ⌀ | Measure one test print; it is the largest single error source |
| Horizontal hole shrink | 0.1–0.2 mm, less than vertical | Elliptical holes are normal; design round, expect oval |
| M3 clearance hole | ⌀3.4 | ⌀3.2 for a snug fit |
| M3 self-tapping pilot | ⌀2.4–2.5 | Good twice, loose the third time: prefer inserts |
| M3 heat-set insert | hole ⌀ = insert OD − 0.1, depth = insert length + 0.5 | Brass insert OD is usually 4.6 mm → ⌀4.5 hole; add a 45° lead-in |
| Nut trap | across-flats + 0.2, depth = nut thickness + 0.2 | Leave a slot to push it in, or a closed pocket if it goes in from the side |
| Screw boss OD | **insert boss = insert OD + 4** (⌀4.6 insert → ⌀8.6); self-tapping boss = 2 × screw ⌀ (M3 → ⌀6) | Size it from the hole that is in it, never from the screw: a ⌀4.5 insert hole in a ⌀6 boss leaves 0.75 mm of wall and splits when the insert goes in. Rib it to the wall rather than thickening the wall: 3 ribs, 1.5 mm thick, 45° taper |

- **Do not model threads below M8.** Printed threads work only at coarse pitch and large diameter; use an insert, a self-tapper or a nut trap.
- **A boss taller than 3 × its OD wobbles** and the screw cams out. Cap it with a rib or shorten it.
- **Washers and screw heads need clearance above the boss**, and the head must not bottom out before the joint closes: counterbore depth = head height + 0.2.

## Overhangs, bridges and supports
- **45° from vertical prints without support.** Anything shallower needs support or a redesign; a chamfer at 45° on the underside is almost always better than support because the finish is better and nothing has to be cut away.
- **Bridges work up to about 10 mm** cleanly, 20 mm with sag, 30 mm and beyond are unreliable. A flat roof over a cavity is a bridge: split it, chamfer it, or add a mid support.
- **Horizontal holes sag at the top.** Above ⌀6 mm the roof of a horizontal hole is an overhang: use a **teardrop** (sides at 45° meeting at the top) or a **diamond** for anything that must stay round, or accept a 0.3–0.5 mm flat and drill it out.
- **Design so the support-free orientation works**: put the largest flat face on the bed, keep functional surfaces vertical or upward-facing, and orient holes so their axes are horizontal only where the sag does not matter.
- **Support leaves scars.** If a face must be supported, make it a non-cosmetic, non-mating face.

## Orientation, strength and material
- **Layer adhesion is the weak direction.** A part loaded across its layers delaminates at a fraction of the in-plane strength. Orient so tension and bending run *within* the layers, and say which way that is in your reply.
- **Heat**: PLA creeps at ~55 °C — unsuitable next to a PSU, a hotend or in a car. PETG to ~75 °C, ASA/ABS to ~95 °C. A part that holds something hot is not a PLA part.
- **Warping** comes from long straight walls, large flat areas and uneven thickness. Break long walls with ribs or a slight curve, keep thickness uniform, and prefer PETG or PLA over ABS unless heat demands it.
- **Clearance to heat sources**: keep 10 mm between a printed wall and a PSU or heatsink where the airflow allows it.

## Splitting and assembly
- **Split at a plane, not at a feature.** Two parts that join through a screw boss cannot be printed and aligned well.
- **Every joint needs alignment as well as fastening**: dowel pins (⌀6 × 8 mm, 0.2 mm clearance), a lap or a dovetail, plus M3 screws or clamps. Screws alone locate nothing.
- **Glue faces need 3 mm of width** and a rough surface; solvent weld (ABS in acetone, dichloromethane for PETG/PLA) is stronger than cyanoacrylate but messier.
- **Leave a screwdriver path**: every screw must be reachable with the assembly in its final order, or the part cannot be serviced. Write the assembly order out and check each step.
- **Label the parts** with embossed letters or numbers on a hidden face; identical-looking halves are a classic assembly failure.
- **Add a 1 mm assembly allowance** around moving sub-assemblies (a fan, a door) so a slightly swollen print still fits.

## Verification before saying it prints
Run these in the design, not from memory. `inspect_design {measure:true}` and `inspect.measure` give the numbers; a `run_script` session verifies without touching the user's design.
1. **Build volume**: every printed body's bounding box ≤ the volume, with 5 mm to spare on X and Y for a brim.
2. **Wall thickness**: measure the thinnest wall of each part; it must be ≥ 1.6 mm for anything structural, ≥ 1.35 mm for anything at all.
3. **Fits**: for each mating pair, measure the gap and compare with the table. Zero gap where parts must move is a bug, not a tight fit.
4. **Fasteners**: every hole's ⌀ and depth against the table, and every boss OD against the hole in it — insert boss ≥ insert OD + 4, self-tapping boss ≥ 2 × screw ⌀.
5. **Overhangs**: list every face steeper than 45° from vertical that is not on the bed; each one is support or a redesign.
6. **Holes**: every horizontal hole above ⌀6 is teardropped, drilled out or accepted with a stated flat.
7. **Counts and clearances inside cavities**: the drives, the board, the fan — measure the space they need against the space they have, and state both numbers.
8. **Assembly order and access**: write the order down and check screwdriver access at each step.

Report the numbers you measured, not the ones you intended: "cooler clearance 162.4 mm measured against 160 required" beats "enough room for the cooler".

## Traps
1. **A hole that measures right and prints small.** Vertical holes shrink in every FDM process; a ⌀3.4 hole can arrive at ⌀3.1. Compensate once, in the parameter, per printer.
2. **Zero-clearance joints.** Two parts modelled touching cannot be assembled, and the slicer does not care. If the design has a lid, a tray or a drawer, it needs a gap parameter.
3. **Bosses that split.** A ⌀6 boss with a ⌀2.5 pilot leaves 1.75 mm of wall each side; the same boss with a ⌀4.5 insert hole leaves 0.75 mm and cracks as the insert is pressed in. Keep at least 2 mm of material around the hole, size the boss from the hole, and rib it.
4. **Thin tall walls** (a 1 mm panel 80 mm high) warp, ring and snap. Rib them or thicken them.
5. **Sharp inside corners** concentrate stress and are the first place a part cracks. Fillet them.
6. **Support in a mating surface.** The support scar leaves a 0.2 mm bump, and the parts rock instead of seating. Move the split.
7. **A part that fits the bed diagonally but not squarely**, or that fits with no room for a brim or a skirt. Measure the bounding box, not the length.
8. **Heat near plastics** ignored because the part is only "near" the PSU, not touching it. PLA sags under its own load at 60 °C.
9. **Screws that cannot be inserted** because the head is inside a cavity with no tool access, or the screw enters from behind a wall.
10. **Assuming the printer's tolerance** instead of asking. A machine that prints ⌀3.4 at ⌀3.15 needs different numbers than one that prints it at ⌀3.35, and only the user knows which they have.
