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
| Open CASCADE (OCCT) 8.0.1, experiment | `crates/kernel` (`backend/occt.rs`), over `vendor/occt-capi` | SolidCraft (experiment) | `backend-occt` | box, cylinder, sphere, torus, boolean, fillet (constant radius, mitred turns) | fails: 168 of 184 corpus designs (solidcore 181) | fails: 2 of 23 cases looser (at most 2.2e-13), 0 wrong |

The Open CASCADE row is an experiment that checks the interface fits a second kernel. It is not a
kernel we plan to route anything to. OCCT is LGPL-2.1 with the Open CASCADE exception: it lives in
its own shared library, linked dynamically and built only with the feature, so no build we ship
holds it (`vendor/occt-capi/README.md`). Bodies cross to and from it as STEP, written and read by
solidcore on our side, and its results carry no history yet.

### Open CASCADE, measured 2026-11-27

`cargo run --release -p solidcraft-kernel --features backend-occt --example occt_matrix` builds
each case through the router on both kernels and grades it against its closed-form volume and
area and the shared check (OCCT 8.0.1, `main` at 339a5119f5):

| Operation | Cases | solidcore pass | OCCT pass / fail / unsupported | OCCT looser than solidcore (any / above 1e-12) |
|---|---|---|---|---|
| box | 3 | 3 | 2 / 0 / 1 | 0 / 0 |
| cylinder | 3 | 3 | 2 / 0 / 1 | 1 / 0 |
| sphere | 2 | 2 | 2 / 0 / 0 | 0 / 0 |
| torus | 3 | 3 | 2 / 0 / 1 | 0 / 0 |
| boolean | 9 | 9 | 8 / 0 / 1 | 1 / 0 |
| fillet | 3 | 3 | 3 / 0 / 0 | 0 / 0 |
| **all** | **23** | **23** | **19 / 0 / 4** | **2 / 0** |

- Unsupported means solidcore ran it. Three are refusals of bad input, which the adapter hands
  to solidcore so the words match. The fourth is a union of apart cubes, which the STEP crossing
  returns as two parts.
- The two looser cases are the cylinder along (1, 1, 1) (volume 2.2e-13, area 1.6e-13 relative,
  against 0 and 1.4e-16) and the crossing cylinders' area (1.0e-14 against 3.9e-16): the rounding
  of the STEP crossing. The accuracy gate takes no looser case, so it fails here.
- The worst tolerance the records declare is 1e-7 on OCCT results (OCCT's STEP uncertainty, as
  solidcore's reader records it) against 2e-5 to 8e-5 on solidcore's.

**Naming corpus** (`cargo test --profile ci -p solidcraft-cli --test naming_corpus --features
solidcraft-kernel/backend-occt`, with `SOLIDCRAFT_KERNEL` set to each kernel): 181 of 184
designs pass on solidcore, whose route runs the code a build without the feature runs. 168 pass on OCCT,
and there their two builds name alike and every stored name finds its face or edge. Of the 13
more that fail on OCCT:

- 11 stop at a cylinder, whose callers need the construction sources solidcore gives its faces
  (OCCT results carry none);
- 1 sketch on a face can't keep its attachment without history;
- 1 sketch offset meets a degenerate piece.

The naming gate takes no new failure, so it fails too.

**The kernel's own tests** (`cargo test -p solidcraft-kernel --lib --features backend-occt`, the
tests that switch kernels left out): 1240 of 1245 pass on solidcore and 1099 on OCCT. Most of
the 146 that fail on OCCT ask for history, which OCCT results don't carry yet. The rest fail in
OCCT fillets, or in solidcore operations on OCCT-made bodies.

So the interface took a second kernel without changes to the trait, and the gaps are in what the
adapter carries back: history, construction sources and apart solids.

### Known limitations

- The model cache saved in a design and the import cache don't record the kernel that computed them, so the opt-in `backend-occt` build can share cached bodies with a normal build of the same commit ([#23125](https://github.com/tangency-labs/solidcraft/issues/23125)).
