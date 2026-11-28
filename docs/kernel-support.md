# Kernel support matrix

SolidCraft asks a kernel for bodies through one interface, the `Backend` trait in
`crates/kernel-api`; `crates/kernel` is the router behind it ([design](design/kernel-layer.md)).
This page lists every kernel with an adapter to that interface.

We write and maintain one adapter, solidcore's, over `solidcore-ops`. It is the default, and the
only kernel in any build we ship. Speed and features are fixed inside solidcore.

Anyone who wants another kernel in SolidCraft writes and maintains its adapter to the interface,
behind a build feature that is off by default. It is listed here, and it is routed for an
operation only when it passes, on that operation's cases:

- **the naming gate:** identical verified names on every corpus design that uses the operation;
- **the accuracy gate:** on every case, a tolerance equal to or tighter than solidcore's, after the
  exchange, on the whole body (design §8, rule 5).

Which kernels a build holds is decided at compile time. Choosing among them at run time comes
later.

| Kernel | Adapter | Maintained by | Build feature | Operations | Naming gate | Accuracy gate |
|---|---|---|---|---|---|---|
| solidcore | `crates/kernel` (`Solidcore`) | SolidCraft | always on | all (`OpKind::ALL`) | the reference | the reference |
| Open CASCADE (OCCT) 8.0.1, experiment | `crates/kernel` (`backend/occt.rs`), over `vendor/occt-capi` | SolidCraft (experiment) | `backend-occt` | box, cylinder, sphere, torus, extrude (not tapered) and revolve of sketch profiles, boolean, fillet (constant radii, mitred turns, the Fillet feature's too), equal chamfer, shell, thicken, offset faces, draft, loft (two sections, or a section to a point; no rails), split of a body by a plane (one piece each side), trim of a sheet by a plane | fails: 179 of 185 corpus designs (solidcore 182) | fails: 2 of 30 cases looser (at most 2.2e-13), 0 wrong |

The Open CASCADE row is an experiment that checks the interface fits a second kernel; no build we
ship routes anything to it. OCCT is LGPL-2.1 with the Open CASCADE exception: it lives in
its own shared library, linked dynamically and built only with the feature, so no build we ship
holds it (`vendor/occt-capi/README.md`). Bodies cross to and from it as STEP, written and read by
solidcore on our side. Its extrudes and revolves carry the same history and construction sources
as solidcore's: solidcore plans the profile, OCCT sweeps exactly those curves, and the router names
the faces that come back with solidcore's own naming, running solidcore instead (with the reason
logged) where they don't name alike. Its other results carry no history yet.

### Open CASCADE, measured 2026-11-27

`cargo run --release -p solidcraft-kernel --features backend-occt --example occt_matrix` builds
each case through the router on both kernels and grades it against its closed-form volume and
area and the shared check (OCCT 8.0.1; the primitives, booleans and fillets at `main` 339a5119f5,
extrude and revolve at 058b6c9807):

| Operation | Cases | solidcore pass | OCCT pass / fail / unsupported | OCCT looser than solidcore (any / above 1e-12) |
|---|---|---|---|---|
| box | 3 | 3 | 2 / 0 / 1 | 0 / 0 |
| cylinder | 3 | 3 | 2 / 0 / 1 | 1 / 0 |
| sphere | 2 | 2 | 2 / 0 / 0 | 0 / 0 |
| torus | 3 | 3 | 2 / 0 / 1 | 0 / 0 |
| extrude | 4 | 4 | 3 / 0 / 1 | 0 / 0 |
| revolve | 3 | 3 | 2 / 0 / 1 | 0 / 0 |
| boolean | 9 | 9 | 8 / 0 / 1 | 1 / 0 |
| fillet | 3 | 3 | 3 / 0 / 0 | 0 / 0 |
| **all** | **30** | **30** | **24 / 0 / 6** | **2 / 0** |

- Unsupported means solidcore ran it. Five are refusals of bad input (a flat box, a zero radius,
  a torus whose minor radius is above its major, an extrude of zero distance, a revolve of zero
  turn), which the adapter hands to solidcore so the words match. The sixth is a union of apart
  cubes, which the STEP crossing returns as two parts.
- Extrudes and revolves: a rectangle, a plate with a round hole, a disc on a tilted plane with
  whole sides, a tube turned a full turn, a cylinder turned a quarter. Volumes and areas match
  the closed forms to 4e-16 on both kernels. Their parity tests (`backend/occt.rs`) also build a
  profile with a side drawn in two pieces, two regions, two cubic spans of one curve, both as
  drawn and with whole sides, and partial and full turns on and off the axis: each the same
  volume, area, face and edge counts, face history and construction sources on both kernels.
- The two looser cases are the cylinder along (1, 1, 1) (volume 2.2e-13, area 1.6e-13 relative,
  against 0 and 1.4e-16) and the crossing cylinders' area (1.0e-14 against 3.9e-16): the rounding
  of the STEP crossing. The accuracy gate takes no looser case, so it fails here.
- The worst tolerance the records declare is 1e-7 on OCCT results (OCCT's STEP uncertainty, as
  solidcore's reader records it) against 2e-5 to 8e-5 on solidcore's.

**More matrix rows (at f59520a67e):** on every one of these, OCCT passes, with 0 fail and 0 unsupported:
- a revolve of a disc drawn as two half circles, the arcs joined first as the Revolve feature asks;
- a fillet as the Fillet feature rounds it (`fillet_removing`);
- an equal chamfer, a shell open at the top, a thickened sheet, a face moved out, a drafted side, a two-section loft (square to square), and a loft of a square to a point.

Two cases are looser, both below 1e-12: the draft's volume (2.4e-15 on OCCT against 0 on solidcore) and the loft to a point's volume (6.0e-13 against 2.2e-16).

**Chamfer, shell, thicken and offset faces** (`chamfers_shells_and_thickened_sheets_match_solidcore`
in `backend/occt.rs`): a cube's edge chamfered 1 back, the cube shelled 1 thick open at the top, a
10 x 10 sheet thickened 2 up, 2 down and 1 each way, a face moved out 2 and another in 1. On each,
OCCT's result has solidcore's volume (and the closed form) to 1e-9, its area, face and edge
counts and centroid, and passes the shared check. These results carry no history yet, so features
that name their faces find them by geometry only; unequal chamfers, chamfers that carry a face on
and closed hollows stay on solidcore.

**Split and trim by a plane** (`splits_by_a_plane_match_solidcore_piece_by_piece` and
`sheets_trimmed_by_a_plane_match_solidcore` in `backend/occt.rs`, and the matrix's split and trim
rows): a cube cut across, the same plane turned over, a cube cut corner to corner and a cylinder
halved along its axis give, piece by piece and in solidcore's order (the piece on the plane's
back first), solidcore's volume (and the closed form) to 1e-9, its area, face and edge counts and
centroid. Square sheets, one with a hole, trimmed by a plane keep solidcore's area, face and edge
counts. Matrix: split 5 cases, OCCT 4 pass / 0 fail / 1 unsupported (a plane that misses the
cube, solidcore's whole body), 0 looser; trim 2 cases, 2 pass, 1 looser below 1e-12 (area
1.6e-14 against 0 on the sheet with a hole). A plane that misses or nicks the body, several pieces
a side, splits and trims by faces or sketch curves, trims keeping picked regions and trims of
solids stay on solidcore. Pieces and trims carry no history yet.

**Naming corpus** (`cargo test --profile ci -p solidcraft-cli --test naming_corpus --features
solidcraft-kernel/backend-occt`, with `SOLIDCRAFT_KERNEL` set to each kernel, at 650f436b37): 182
of 185 designs pass on solidcore, whose route runs the code a build without the feature runs. 179
pass on OCCT, with every primitive, extrude, revolve, boolean, constant fillet, equal chamfer,
shell, thicken and face offset they make built there, and there their two builds name alike and
every stored name finds its face or edge. OCCT cylinders, spheres and tori carry solidcore's face
history and construction sources (a cylinder is solidcore's circle extruded on OCCT and named by
solidcore's cylinder naming). Two designs solidcore can't build (a shell and a thicken its offset
refuses) build on OCCT. Five fail on OCCT only:

- 2 variable fillets (solidcore's) fail on a body OCCT made;
- 1 rectangular pattern's boolean (solidcore's) fails on OCCT-made bodies;
- 1 join is handed a surface body where solidcore's build has a solid;
- 1 sketch on a face can't keep its attachment without history.

Any OCCT failure but a stop falls back to solidcore with the reason logged.

The naming gate takes no new failure, so it fails too.

**The kernel's own tests** (`cargo test -p solidcraft-kernel --lib --features backend-occt`, the
tests that switch kernels left out, at 058b6c9807): 1264 of 1269 pass on solidcore (the five
others fail on `main` too, https://github.com/tangency-labs/solidcore/issues/3445) and 1110 on
OCCT. Most of the 159 that fail on OCCT ask for the history of a boolean or a fillet, which OCCT
results don't carry yet; the rest fail in OCCT fillets, or in solidcore operations on OCCT-made
bodies.

So the interface took a second kernel without changes to the trait, and the gaps are in what the
adapter carries back: history, construction sources and apart solids.

### Known limitations

- The model cache saved in a design and the import cache don't record the kernel that computed them, so the opt-in `backend-occt` build can share cached bodies with a normal build of the same commit ([#23125](https://github.com/tangency-labs/solidcraft/issues/23125)).
