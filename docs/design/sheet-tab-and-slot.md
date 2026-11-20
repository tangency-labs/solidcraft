# Tab and Slot on sheet metal (#11398)

Tabs on one sheet that pass through matching slots in another part, so sheet metal parts register with each other, hold themselves in place during fit-up, and go together only one way. This is the sheet metal route of the plate joint tool started in #12261 (finger joints between solid plates). It is one tool. The route is picked from what is selected.

## Goals

- Pick a **rip joint** of a sheet (the edge where two of its panels are cut apart, as at a closed corner): tabs grow from one panel across the rip into the other.
- Pick an **open edge** of a sheet: tabs grow from that edge into whatever part they reach, another sheet or a solid.
- Work on any panel the sheet has, flat or rolled. Nothing in this route asks what kind of surface a panel is. It works through the panel's own surface coordinates, so a panel kind added later (#15743, developable panels with no closed-form flat) gets tabs with no change here.
- Seeded width randomization, so each joint fits only one way.
- Corner breaks on the tab tips, so tabs start into their slots instead of catching.
- The flat pattern shows the tabs, slots and corner breaks, because they are part of the sheet.

## How it works

This follows the surface-first model of #6748: the sheet (its panels on their surfaces and the joints between them) is the master, and the folded body and the flat are built from it.

1. **Lay out the tabs.** Measure the picked edge's length along the panel's surface and call the shared layout function (below). It returns the tab spans along the edge.
2. **Grow the tabs.** Each span of the edge moves out from the panel by the tab depth, square to the edge on the panel's own surface. A rip lap is the same operation with the opposite sign, so a tab on a roll follows the roll and a tab on a plane stays flat. The panel's outline changes; no boolean touches the sheet, so panels that share a surface keep their joints.
3. **Cut the slots.** Each tab, made wider by the slot width clearance and thicker by the slot thickness clearance, is a tool body cut from the receiving part. On a sheet the cut goes through the existing cut reading (`sheet_surface_cuts`, `sheet_surface_shadow`), which turns it into a clean cut straight through the thickness as a shop would cut it in the flat. Without that step, slots through rolls come out as wedges. On a solid the cut is a plain boolean.
4. **Lap rips.** Across an unlapped rip, the receiving panel laps onto the tab panel by the rip lap distance, except where a tab crosses, so the tabs have material to pass through.
5. **Break the tip corners.** The two tip corners of each tab next to the wall get a chamfer of the tip chamfer width, in the panel's outline.
6. **Rebuild.** The sheet is built again (`rebuild_surface_sheet`) and its faces named from what made them.

The feature finds its sheet and edges as hems and edge flanges do: through the model's names for the picked edges (`<face A>|<face B>`, format v62), then `surface_edge_pick`. A picked edge along a rip is the joint case; any other free edge is the open-edge case. One list of edges covers both.

## Shared with the plates route

One feature record, one command and one dialog for both routes. The record has a route choice:

- `Plates { bodies }`: #12261, solid plates, box cutting tools.
- `Sheet { edges }`: this route.

Until this route lands, a sheet metal pick answers "the sheet metal route is not built yet", not "cannot be cut".

The **layout function** is pure and works on one length, so both routes call it: the plates route with an overlap measured on a plate, this route with a length along a curved edge. Its input is the length and the layout settings; its output is the tab spans as (start, end) along it. Spacing, as OnlyTabs has it:

- **Equal:** a count, spread evenly, with gaps or tabs at the ends.
- **Distance:** a pitch from one tab's center to the next.
- **Best fit:** a target pitch, rounded to a count (optionally even or odd only).
- **Offsets:** keep-out lengths at one or both ends.
- **Closed edges** (a disc's rim) have no ends: the tabs go round one pitch apart.

The function is `joint_layout::lay_out` in `crates/doc`. A finger joint (#12261) is equal spacing with tabs at the ends and gaps as wide as the tabs: for `n` fingers, `n + 1` tabs of width length / (2n + 1). One plate keeps the spans and the other keeps the gaps between them.

**Randomization** changes each tab's width, never its position: each tab keeps its center, and its width is the nominal width plus a value between minus and plus the variation, drawn from the seed and the tab's index by a fixed generator (SplitMix64). A width never goes below 10% of nominal. A tab at an end of an open length that grows past the end stops there. The feature refuses a variation equal to or larger than the smallest gap between tabs (on a loop, counting the gap across its closing point), so whether a joint builds never depends on the seed. The record keeps an on/off switch, the seed (default 1000) and the variation; the dialog has a "new seed" button.

**Tip chamfer width:** default 0.5 mm, 0 for none. Each route applies it to its own geometry.

## Settings

| Setting | Default | Notes |
|---|---|---|
| Tab width | 25 mm | nominal, before randomization |
| Tab depth | 1.3 mm | how far each edge span moves out to make its tab |
| Spacing | to settle with #12261 | see the layout function |
| Slot width clearance | 0.25 mm | each side of a tab |
| Slot thickness clearance | 0 | room between a tab's faces and its slot |
| Rip lap distance | 0.76 mm | 0 leaves rips as they are |
| Tip chamfer width | 0.5 mm | 0 for none |
| Randomize widths | off | seed 1000, variation 2.5 mm |

The defaults are OnlyTabs' (1 in, 0.05 in, 0.01 in, 0, 0.03 in, 0.02 in, 0.1 in) in millimeters.

## Refusals

Each names the edge or tab and why, in the feature's report:

- a variation equal to or larger than the smallest gap;
- a tab that reaches no part (open edge);
- a slot the receiving sheet cannot take as a straight-through cut (for example one that would stop inside a bend);
- a tab span shorter than its two tip chamfers.

## Order of work

1. The layout function and its tests, shared with #12261.
2. Tabs and slots across a rip, on flat and rolled panels.
3. Tabs on open edges, into another sheet or a solid.
4. Randomization and tip chamfers on this route; the flat pattern check.
5. The sheet route in the shared record, command and dialog, once #12261 has landed.

## Tests

- Layout: spans for each spacing mode against values worked by hand; randomization keeps every center; the same seed gives the same widths; no seed overlaps tabs under the refusal rule.
- Geometry: a box corner with a rip, flat; the same on a rolled panel meeting a flat one; an open edge into a solid plate. Each checks the folded volume, that each slot clears its tab by the clearances, and that the flat pattern's outline has the tabs and slots.
