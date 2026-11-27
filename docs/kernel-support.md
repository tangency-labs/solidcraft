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
