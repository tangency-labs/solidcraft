# A swappable kernel layer in SolidCraft

Status: approved by the owner in full. He approved phase 1, the seam (§12), on 2026-11-24, and on 2026-11-26 every later phase of §12 as well: "I asked for it". Phase 1 is the kernel interface in SolidCraft, the solidcore adapter behind it, and SolidCraft calling the kernel only through that interface. No second kernel is planned or being built. The later sections describe what the seam must allow, so that the interface is right the first time; they are not a plan to add another kernel.

The owner's brief (2026-11-24):

> solidcore/ops is the solidcore one... then we have one in SOLIDCRAFT NOT SOLIDCORE that is where
> the kernel can be selected, then we make things that can adapt kernels to that abstraction.

Later the same day the owner added these requirements:

- the kernel is chosen at compile time: build features decide which kernels are compiled in;
  choosing among them at run time comes later, not now;
- SolidCraft must use `solidcore-ops` fully, and only through the solidcore adapter.

What follows from that:

- **solidcore stays an independent, app-agnostic kernel.** `solidcore-ops` is solidcore's own
  operations API. It gains no trait, no "backend" and no SolidCraft type. Work this design asks
  of solidcore is new app-agnostic operations in `solidcore-ops`. Each one is documented and goes
  in its own `solidcore/` pull request (§10).
- **Speed and features keep being fixed inside solidcore.** This layer is not a way around
  solidcore's slow cases or missing features. Those are solidcore's to fix, in solidcore, and that
  stays the kernel work's focus. Nothing here moves effort away from it.
- **SolidCraft owns the interface.** One trait, in `crates/kernel-api`, says what SolidCraft needs
  from a kernel. `crates/kernel` is the router behind it.
- **We maintain exactly one adapter: solidcore's.** It adapts `solidcore-ops` to the interface. We
  write it and keep it in SolidCraft, and it is the default.
- **SolidCraft carries no compatibility layer for other kernels.** If anyone else wants their
  kernel in SolidCraft, they write and maintain its adapter to our interface. To count, it must
  pass our naming gate and an accuracy gate with a tolerance equal to or tighter than solidcore's
  (§8), and it is listed in the kernel support matrix (§8). None is planned.
- **The solidcore adapter is the only SolidCraft code that depends on solidcore crates.** The one
  exception is the record's data types, which are the body format itself (§10). `cargo xtask
  layers` enforces this, as it once kept truck inside `crates/kernel`. This also helps solidcore:
  every call SolidCraft makes goes through `solidcore-ops`, so its public API ends up complete and
  documented, which is what makes solidcore usable as a general kernel.
- **Kernels are chosen at compile time.** Build features decide which kernels are compiled in.
  solidcore is always in, and is the only one in any build we ship. Choosing a kernel at run time,
  and swapping one while the app runs, come later, not in phase 1.
- **STEP stays in solidcore.** Import and export are solidcore's, reached through `solidcore-ops`.
- **solidcore stays the default,** and the default path stays byte-identical to today.

## Decisions

Decisions the owner has made. The rest of this document follows them; the open questions are in
§13.

| Date | Decision |
|---|---|
| 2026-11-24 | **The abstraction is SolidCraft's, not solidcore's.** `solidcore-ops` stays solidcore's own app-agnostic API, and the trait and the router live in SolidCraft (§1). |
| 2026-11-24 | **SolidCraft uses `solidcore-ops` fully, and only through the solidcore adapter.** In the end state the adapter is the only SolidCraft code that depends on solidcore crates, and `cargo xtask layers` enforces it (§10). |
| 2026-11-24 | **Kernels are chosen at compile time, by build features.** Features decide which backends are compiled in. solidcore is always compiled in; every other backend is off by default. The launch-time switch (`SOLIDCRAFT_KERNEL`, `SOLIDCRAFT_KERNEL_ROUTE`, `--kernel`) picks only among the backends compiled in (§8). **Hot swap is not in v1:** changing kernels takes a restart (§11). |
| 2026-11-24 | **solidcore is the default backend,** and the default path stays byte-identical (§4). |
| 2026-11-24 | **Build it: phase 1 only.** Phase 1 is the seam: the `crates/kernel-api` trait, `crates/kernel` as the router, the solidcore adapter, and SolidCraft moved to call the kernel only through it, with a check that fails on new direct uses of solidcore outside the adapter. No second kernel (§12). |
| 2026-11-24 | **We write and maintain the solidcore adapter, and only it.** It adapts `solidcore-ops`, solidcore's own API, and lives in SolidCraft. Anyone else writes and maintains their own kernel's adapter to our interface; it must pass our naming gate and an accuracy gate (tolerance equal to or tighter than solidcore's) and appear in the support matrix (§8). |
| 2026-11-24 | **Run-time selection comes later.** In phase 1, build features alone decide which kernels are compiled in, and solidcore is the only one. The launch-time switch, per-operation routes and hot swap described below are later work. This supersedes the launch-time switch in the compile-time decision above. |
| 2026-11-24 | **STEP stays in solidcore** (§2, §10). |
| 2026-11-26 | **Build all of it.** Every later phase of §12 is approved too (steps 6 to 16: smlib onto `solidcore-ops`, the adapter split, sweep and loft in the trait, exchange, measurement, run-time selection and designs recording their kernel). Each step still follows §12's rules. Still no second kernel. |

Contents:
0. [Decisions](#decisions)
1. [Where the abstraction lives](#1-where-the-abstraction-lives)
2. [The trait surface](#2-the-trait-surface)
3. [The body exchange format](#3-the-body-exchange-format)
4. [Naming](#4-naming)
5. [Designs record their kernel](#5-designs-record-their-kernel)
6. [Errors](#6-errors)
7. [Checks](#7-checks)
8. [Selection, routing and measurement](#8-selection-routing-and-measurement)
9. [Performance and binary cost](#9-performance-and-binary-cost)
10. [solidcore only through ops, only through the adapter](#10-solidcore-only-through-ops-only-through-the-adapter)
11. [Switching kernels with a design open](#11-switching-kernels-with-a-design-open)
12. [Migration](#12-migration)
13. [Risks and open questions](#13-risks-and-open-questions)

---

## 1. Where the abstraction lives

### Today

- **`crates/kernel` (`solidcraft-kernel`) is already the app's kernel boundary.** Its crate doc
  says no solidcore type appears in its public API, "so the kernel can be replaced without
  touching the rest of the workspace".
- **But it isn't the only door.** Thirteen other SolidCraft crates depend on solidcore crates
  directly (§10). Two of them, `crates/smlib` and `crates/geom`, reach the kernel crate itself
  through `xtask/solidcore-seam.txt`.
- **Every operation already runs on the record and comes back as one.** A `Body` *is* its
  schema 4 record (`solidcore_record::brep::Brep`, in canonical order). The solidcore forms
  (`Body::smlib`, `Body::ops`) are caches built from that record on first use.
- **The adapter pattern already exists.** `crates/kernel/src/offset_backend.rs` and
  `smlib_boolean.rs` already do what an adapter must do:
  1. send the record in, tagging each face and edge with its record index;
  2. run the operation;
  3. read the tags back as an exact `History`;
  4. renumber the result into the record's order.
- **The surface is large.** On `origin/main` the app uses about 322 distinct `solidcraft_kernel`
  items from outside the crate, in roughly 2,990 qualified references. `Body` has about 95 public
  methods, about 85 of them called from outside.

### End state: the crates

| Crate | Holds | Depends on |
|---|---|---|
| `crates/kernel-api` (`solidcraft-kernel-api`, new) | the `Backend` trait, `OpKind`, `Route`, `Made`, `ExchangeLoss`, `BackendError`, and the neutral input types (profiles, path segments, extents) | `solidcore-record` (its data types only, §10), `thiserror` |
| `crates/kernel` (`solidcraft-kernel`, as now) | the **router**: the same public functions as today, each picking its backend from the route; the `Body` type and its caches of backend forms; the record services' public face | `kernel-api`, and each adapter crate (solidcore's always; the others behind features) |
| `crates/kernel-solidcore` (new; today's solidcore code in `crates/kernel` and `crates/smlib`, moved) | the solidcore adapter: `impl Backend for Solidcore`, plus every call SolidCraft makes into `solidcore-ops`, `-tess` and `-step` | `kernel-api`, the solidcore crates |
| `crates/kernel-<name>` (only if someone brings another kernel; none is planned) | that kernel's adapter, written and maintained by whoever brings that kernel, not by us | `kernel-api`, that kernel |

### Why the trait doesn't go in `crates/kernel`

- **It would make a dependency cycle.** An adapter implements the trait, so it depends on the
  trait's crate. The router calls the adapters, so it depends on them. If the trait lived in
  `crates/kernel`, the two would depend on each other.
- **It would also make `crates/kernel` the place every adapter reaches into.** A trait in a small
  crate of its own gives each adapter only the contract it needs.
- **`crates/kernel` only shrinks.** CONTRIBUTING ("Kernel work goes in solidcore/") says so. Here
  it shrinks to a router once the solidcore code moves to `crates/kernel-solidcore` (§12, step 10).

### Why the router stays in `crates/kernel`

- **Callers don't change.** Every caller already calls `solidcraft_kernel::fillet`, `boolean`,
  `extrude` and so on. Keeping the router behind those names means no call site in doc, engine or
  io changes for the router itself.
- **The only caller changes are the bypasses.** Code that today reaches `solidcore-ops` directly
  is switched to `crates/kernel`'s re-exports, one crate per pull request (§10).

### Why not put it in `crates/engine` or `crates/doc`

- **`crates/doc` calls the kernel directly.** About 55% of the distinct items it uses (most of
  the blend, sweep, loft, surface and edit calls) are used only from doc. A layer above doc would
  miss them.

### Documentation and layer checks

The layer table in `docs/architecture.md` (L1) gains the new crates. `cargo xtask layers` enforces
these rules:

- adapters depend on `kernel-api` and their own kernel, never on `crates/kernel` or on each other;
- only `crates/kernel` depends on adapters;
- only `crates/kernel-solidcore` depends on solidcore crates, apart from the record's data types
  (§10).

---

## 2. The trait surface

### What the app calls today, grouped

The counts below come from a grep of qualified references outside `crates/kernel` on
`origin/main`. They cover `solidcraft_kernel::X` and the `kernel::X` alias that 14 doc files use.
They include tests and `use` lines, so they show how far each item reaches, not exact call counts.

| Family | Main items (references) | Called from |
|---|---|---|
| Primitives | `box_solid` 111, `cylinder` 43, `sphere` 10, `torus` 5, `half_space` 11 | doc, engine, io, gpu, ui |
| Extrude, revolve | `extrude` 31, `extrude_tapered(_between)` 4, `extrude_planar_regions` 3, `revolve` 9, `revolve_whole` 1 | doc, engine |
| Sweep, loft | `sweep(_with)` 4, `sweep_helix(_radial)` 3, `sweep_point_path_with` 2, `loft*` 7, `loft_blend` 2 | doc |
| Booleans, split | `boolean` 55, `BoolOp` 37, `planar_regions_boolean` 25, `union_all` 4, `union_apart` 6, `join_apart` 4, `split_*` 7, `trim_keep` 3 | doc, engine, io |
| Transform | `transform_matrix` 49, `transform` 5, `place_matrix` 4 | doc, engine, io |
| Blends | `fillet` 4, `fillet_*` variants 14, `chamfer_*` 5, `blend_edges` 5, `tangent_chain*` 5, fit notes 20 | doc, engine, ui |
| Offset family | `offset_faces` 6, `shell` 1, `thicken` 1, `draft` 2, `draft_corners` 3, `offset_surface` 3 | doc |
| Surfaces | `extend_surface` 4, `boundary_*` 5, `patch_*` 3, `ruled_surface_with` 1, `stitch`/`sew` 4, `untrim` 1, `match_surface` 1, `fair_faces` 1 | doc, engine |
| Direct edit | `delete_faces`, `remove_faces`, `copy_faces`, `reverse_faces`, `repair` 2 | doc |
| Queries | `measure` 375, `PointClass` 212, `measure_mass` 29, `face_surface` 57, `edge_circle` 17, `cylinder_face_at` 16, `body_distance` 6, `edge_distance` 9, continuity 40+, combs 6 | everywhere |
| Display | `MeshTolerance` 154, `mesh_body` 20, `Paint` 12, and `Body::mesh*` | doc, io, engine, ui, gpu |
| Import, export | `step_import*` 25, `step_export*` 25, `iges_*` 13, `records_import*` 14, `encode/decode_native_body` 12 | io, engine, cli |
| History, naming | `history::{FaceOrigin, EdgeOrigin, Change}` 13, `FaceSource` 57, `Body::history` 37, `Body::face_sources` 35, `with_*_certificate` 10 | doc, engine |
| Checks | `Validation` 102, `validity` 4, `check_kept` 10, `heals_tolerance_defects` 14 | doc, engine, io, cli |
| Errors | `KernelError` 87, `ErrorCode` 12 | doc, engine, io, ui |
| Runtime | `guard` 22, `map_on_threads` 16, `kernel_thread` 8, `for_each_on_threads` 8, `hold_a_kernel` 6, `Stop` 6, `start` 3 | all |

On top of these, the code that bypasses `crates/kernel` today (§10) calls about 60 more
`solidcore-ops` items, mostly 2D curve fitting, projection, hatching, affine maps and sheet
development.

### Three kinds of call

1. **Body-making operations** take bodies or profiles and return new bodies with a `History`.
   These are the trait, and they are routed per operation.
2. **Record services** read a body's record and decide nothing about its shape: queries and
   measures, point classification and picking, tessellation, the validity check, and the 2D and
   curve helpers the bypasses use. STEP and IGES are treated the same way:
   - **They live in solidcore and stay there.** Import and export are solidcore's
     (`solidcore-step`), reached through `solidcore-ops`, and this design doesn't move them.
   - **They are shared only in one sense.** A body made by another backend is exported by
     converting it to the record and running solidcore's STEP or IGES writer on that record. An
     imported body is read by solidcore, and another backend receives it as a record, like any
     other operand.
   - In v1 they run on solidcore for every backend. A body made by another backend is a record
     like any other, so they work on it unchanged.
   - This keeps the comparison fair: every backend's result is measured, meshed, picked and
     checked by the same code, so the comparison in §8 compares only what the backends made.
   - It also keeps names comparable, because names are worked out from picks and measures.
   - Individual services can join the trait later (a faster classifier, say), each as its own
     `OpKind`, measured like any other.
3. **Runtime services**: guard, threads and stop tokens. The router wraps every backend call in
   `guard`, so a panic in any kernel becomes `KernelError::Internal`.

### v1 operations

A backend states its support per kind, before any conversion: `Support::Yes`, `No`, or
`Some(reason)`.

| Group | `OpKind`s in v1 | Why these |
|---|---|---|
| Primitives | `Box`, `Cylinder`, `Sphere`, `Torus`, `Cone` | about 170 references; easy to compare |
| Profile solids | `Extrude` (blind, between, tapered), `Revolve` (partial, whole) | the most common features |
| Booleans | `Boolean` (union, subtract, intersect; one tool or many) | 55 references, and every cut |
| Blends | `Fillet` (constant radius, mitred corners), `Chamfer` (equal, two distances) | 36 corpus files use fillets, 15 use chamfers |
| Offset | `Shell`, `Thicken`, `OffsetFaces` | 13 corpus files use shells |
| Sweep, loft | `Sweep` (path), `Loft` (sections, rails) | the remaining common solid features |

Transforms (rigid moves, mirror, scale) are **not** routed. They are exact maps of the record
(`placement::moved`). Routing them would gain nothing and would put names at risk.

### Later operations

Each comes later as its own `OpKind`, once v1 is measured:

- **More blends:** variable fillets (points, spline, chord, tapered); G2 fillets and corners;
  setbacks; `fillet_removing`; fit notes and largest-size answers (`*_max_why`).
- **More of the offset family:** `draft` and `draft_corners`; lips (`raise_band`, `rim_band`);
  emboss.
- **Surfaces:** boundary, patch, ruled, extend, trim and untrim, stitch and sew, match, fair.
- **Direct edit and repair:** delete, remove, copy and reverse faces; `repair` and
  `heal_tolerance_defects`.
- **Record services that measure slow:** classification, distance and rays.

### The trait

What follows is the shape, not the final code; step 2 of §12 settles the details.

```rust
/// One kernel, as SolidCraft uses it. Implemented by unit structs (`Solidcore`, and one per
/// other kernel if anyone brings one). The router holds them in a closed enum (§9).
pub trait Backend: Sized + Send + Sync + 'static {
    /// The backend's own body: built from a record, cached on `Body` (§3), shared by clones.
    type Form: Send + Sync + 'static;

    /// The stable name `SOLIDCRAFT_KERNEL`, route files and design files use.
    const ID: BackendId;
    /// What cache keys and design files record: the kernel's version and a digest of its
    /// sources, so a different build of the same kernel is told apart.
    fn version(&self) -> &'static str;

    /// Whether this backend takes `op` on these operands at all (a record kind it can't hold, a
    /// tolerance it can't keep). Asked before any conversion.
    fn supports(&self, op: OpKind, operands: &[&Brep]) -> Support;

    /// Record in: the form, and the map from record indices to the form's entities.
    fn import(&self, record: &Brep) -> Result<(Self::Form, Order), BackendError>;
    /// Form out: a record in canonical order, the map from the form's entities to it, and what
    /// the conversion couldn't carry exactly (§3).
    fn export(&self, form: &Self::Form) -> Result<(Brep, Order, ExchangeLoss), BackendError>;

    fn primitive(&self, p: &Primitive) -> Result<Made<Self::Form>, BackendError>;
    fn extrude(&self, p: &Profile, e: &Extent) -> Result<Vec<Made<Self::Form>>, BackendError>;
    fn revolve(&self, p: &Profile, axis: &Axis2, angle: f64) -> Result<Vec<Made<Self::Form>>, BackendError>;
    fn boolean(&self, op: BoolOp, a: &Self::Form, tools: &[&Self::Form]) -> Result<Option<Made<Self::Form>>, BackendError>;
    /// `edges` are record edge indices of `body`, resolved by the router (§4), never points.
    fn fillet(&self, body: &Self::Form, edges: &[usize], radius: f64, turn: FilletTurn) -> Result<Made<Self::Form>, BackendError>;
    fn chamfer(&self, body: &Self::Form, edges: &[usize], sides: ChamferSides) -> Result<Made<Self::Form>, BackendError>;
    fn shell(&self, body: &Self::Form, open: &[usize], thickness: f64) -> Result<Made<Self::Form>, BackendError>;
    fn thicken(&self, sheet: &Self::Form, distance: f64) -> Result<Made<Self::Form>, BackendError>;
    fn offset_faces(&self, body: &Self::Form, faces: &[usize], distance: f64) -> Result<Made<Self::Form>, BackendError>;
    fn sweep(&self, p: &Profile, path: &[PathSeg], c: &SweepControl) -> Result<Made<Self::Form>, BackendError>;
    fn loft(&self, sections: &[Section], rails: &[Rail], fit: RailFit) -> Result<Made<Self::Form>, BackendError>;
}

/// A result and its history, in the backend's own entity ids. The router renumbers both into the
/// exported record's order (§4).
pub struct Made<F> {
    pub form: F,
    pub history: BackendHistory,
}
```

Three choices in this shape:

- **Operands are the backend's own forms.** The router converts each body once and caches the
  form (§3), so a run of operations on one backend converts once at each end, not once per
  operation.
- **Edges and faces go in as record indices.** The app picks edges by points
  (`fillet(body, &[Vec3], r)`). The router resolves those points with the shared record services,
  exactly as `blend_edges` does today, and passes every backend the same indices. A backend never
  interprets a pick, so a pick can't land on a different edge because another kernel splits or
  orders edges differently.
- **The app's input types move to `kernel-api`.** These are `Plane`, `Region2`, `Loop2`,
  `PathSeg`, `SweepControl`, `FilletTurn`, `ChamferSide` and so on. `crates/kernel` re-exports
  them under their old paths.

### What phase 1 builds of this

Phase 1 has one kernel, so it builds the parts of this shape that the seam needs and no more:

- **The operations take the inputs the app passes today:** points for picked edges and faces, a
  plane and regions for a profile. Resolving picks to record indices belongs with the exchange
  (§12, step 12), where it first matters; until then the solidcore arm stays the exact call it is
  today.
- **Each backend has its own error type,** mapped to and from `BackendError`. solidcore's is
  `KernelError` itself, so the words a user sees can't change; another kernel's errors reach the
  app through `BackendError`.
- **Sweep and loft join the trait later** (§12, step 11): `PathSeg` and `SweepControl` compute
  lengths and tangents of their own, so they can't move into a data-only crate yet.

---

## 3. The body exchange format

### The neutral form is the schema 4 record

Bodies move as a `solidcore_record::brep::Brep`, in two places: between the app and a backend,
and between two backends. That record is SolidCraft's body format. It is schema 4, kept in
canonical order, and it is already what the model cache in a design file stores.

Why the record and not a new format:

- **It is already the body.** `Body` is the record plus caches, and every name, mesh and saved
  model is in the record's order. A second neutral form would be a second implementation of one
  job, which solidcore's layers.md §5 calls a bug. It would also put a conversion on the default
  path, which §9 rules out.
- **It is exact data with no kernel inside.** It holds:
  - analytic surfaces (`Plane`, `Cylinder`, `Cone`, `Sphere`, `Torus`);
  - B-splines and NURBS (`Spline`, `RationalSpline`);
  - revolutions, as their producer holds them (`PlacedRevolution`);
  - corner balls (`BallPatch`);
  - blends by their definition plus a certified net (`Blend { definition }`; see
    `solidcore/docs/definitions-and-certificates.md`);
  - pcurves, and tolerances per body, edge and vertex.
- **Its validator gives one canonical order.** Persistent names follow history in that order.
  Once a backend's history is renumbered into it, which kernel made the body doesn't matter (§4).

The cost is that the trait is tied to the record's schema. A record schema change in solidcore
becomes a change for every adapter, and that pull request updates them. An adapter refuses
(`Support::No`) a schema it wasn't built for.

### Where conversion happens

```text
          record (the Body)                              record (the result Body)
               │  import, once per body per backend, cached          ▲  export, once per result
               ▼                                                     │
  Body ──► Body.forms[backend] ──► backend op(s) ──► Made { form, history } ──► router: renumber
```

- **solidcore forms stay as they are.** `Body` keeps them exactly as today (`smlib`, `ops`).
- **Other backends get one lazily filled slot.** With any other backend compiled in, `Body`
  gains `foreign: Option<Arc<ForeignForms>>`, holding one form per backend. The slot is
  cfg-gated, so a solidcore-only build has the very same `Body` struct as today.
- **A result keeps the form that made it.** A solidcore result's `smlib` cell is seeded when it is
  made, and another backend's result has its form set the same way. Two operations in a row on
  one backend convert the record in once and out once per result; the app needs each result's
  record anyway, for names and display.
- **Backends don't convert directly into each other.** They always go through the record, so N
  backends need N adapters, not N².

### What each backend loses

What matters for naming and saving is the trip into the record. What matters for the backend's
own results is the trip into the backend. The "another kernel" column is written from that
kind of kernel's published design: one linear tolerance, analytic and NURBS geometry, and no
procedural blend definitions. Its adapter pull request confirms it, case by case.

| Record → backend → record | solidcore | loopback (§12, step 12) | another kernel (illustration) |
|---|---|---|---|
| Plane, cylinder, cone, sphere, torus | exact | exact | exact, where it holds the same five in closed form; `PlaneThrough` gets its axes worked out from its three points (one rounding) |
| `Spline`, `RationalSpline` | exact | exact | exact, up to the backend's degree limit; a higher degree is `Support::No` |
| `PlacedRevolution` | exact | exact | exact as its surface of revolution, or as the exact rational form |
| `BallPatch` | exact | exact | exact as a rational net, but the "lies on this sphere" label is lost, so a later offset of it is no longer closed form |
| `Blend { definition }` | exact (S and B) | exact | **the net B only**, as a rational spline, and the definition S is lost. For an exact round, B is S: the geometry is exact but the class label is lost. For a procedural blend, B is within the certified ε. Back in solidcore it is a plain spline that is no longer re-certified as a round, so a later solidcore blend that rolls on it can differ. |
| Pcurves | exact | exact | rebuilt: affine where the backend has an affine pcurve, otherwise fitted within tolerance by the record's own pcurve routine, as every record pcurve off a plane is today |
| Tolerances per edge and vertex | exact | exact | lost where the backend holds one global tolerance. A record with looser edges is `Support::No`, so tolerant imports stay on solidcore. |
| Face sources and certificates (`FaceSource`, `with_*_certificate`) | kept | kept, through history | carried through history (`BodyHistory::face_sources`, §4); not re-proved by the backend |

| Backend → record | another kernel (illustration) |
|---|---|
| Analytic, B-spline and NURBS surfaces and curves | exact |
| Extrusion and ruled surfaces of rational curves | exact, as `RationalSpline` |
| Fitted intersection curves | exact, as the backend's own spline (the record takes it as it is) |
| Its blends | exact as `RationalSpline` where its sections are rational; no `Blend` definition |
| Face names or tags | read as history (§4), then dropped, because the record carries no names |

`ExchangeLoss` is the per-face report of each lossy row above that actually happened. The router
logs it and the rig (§8) counts it per case, so a pass that cost a definition shows as a loss,
not as a clean pass.

---

## 4. Naming

### Today, and what doesn't change

- **History comes from the kernel.** Each operation returns an exact `History`
  (`solidcore_record::brep::history`). For every result face and edge, it gives the origins among
  the operands as `FaceOrigin::{Face, Profile, Cap, Edge, Offset}`, plus a `Change`, in record
  indices.
- **The document turns history into names.** `crates/doc` composes histories along the timeline
  (`lineage.rs`) and turns them into names such as `F5:side:l1` and
  `F2:top#[F2:side:l4,F4:side:l4]` (`history_names.rs`, `naming.rs`). It finds stored names again
  with `name_lookup.rs`.
- **Some names come from geometry.** Faces with no exact history, and split pieces nothing around
  them tells apart, are named from geometry and marked unverified.
- **Edges are named after their faces:** `A|B`, with `~k` where the same two faces meet more than
  once.

None of that changes. The history types are already kernel-neutral data in the record, and the
document never sees a kernel's ids. **The naming rule of this design: every backend reports the
same `History` in record indices, or it doesn't take the operation.**

### How an adapter produces history

This is the tag-and-read pattern that `offset_backend.rs` and `smlib_boolean.rs` already use with
solidcore.

1. **Tag.** Before the call, every face and edge of every operand gets the tag
   `(operand, record index)`, in the backend's own provenance mechanism:
   - solidcore uses its attribute store (`solidcore_ops::history`);
   - a kernel that carries face name sets through its edits (a split face keeps its names, a
     merged face takes the union) gets one synthetic name per face;
   - a kernel that returns a per-operation history of its input faces is read from that.
2. **Run** the operation.
3. **Read.** Turn the tags each result face carries into origins:
   - one tag: `Face { operand, face }`, kept or modified;
   - one tag on several result faces: `Split`;
   - several tags on one face: `Merged`;
   - no tag: a generated face, named by the operation's own role. A profile segment gives
     `Profile { segment }`, a cap gives `Cap { end }`, a blend face gives
     `Edge { operand, edge }` (the record edge the adapter was given), and an offset copy gives
     `Offset { operand, face }`.
4. **Renumber.** `export` returns the map from the backend's entities to the result record's
   canonical indices. The router rewrites the history through it and checks it with
   `BodyHistory::check`: one entry per face and edge, and every origin in range.
5. **Hand over.** `Body::history()` returns it as today, and the document names the body as
   today.

A backend that can't report a role exactly for an operation returns `Support::No` for that
`OpKind`. **No adapter falls back to geometric naming.** That fallback exists for old bodies and
imports, not to make a new backend easier to add.

### Topological naming: why a reshuffling kernel can't break names

A kernel that reshuffles its output (faces in another order, a face split differently, new ids
on every rebuild) is the classic topological naming problem: names tied to a kernel's entities
break when the kernel changes. This design defends against it in three ways:

- **No name ever comes from a kernel id or an order.** Names come only from the `History` each
  operation reports, in record indices, composed by the app (above). A backend that reorders
  faces changes nothing: its history is renumbered into the record's canonical order before the
  app sees it.
- **The naming gate is the defence.** A backend takes an operation only if it reports a `History`
  that gives identical verified names on every corpus design using that operation (rule 2 of §8,
  and `NAMING_CORPUS_KERNEL`). A backend that splits or merges faces differently fails that gate
  for that operation and is not routed for it.
- **Routing is deterministic.** The route is fixed for the session and recorded in the design
  (§5). The same design therefore replays every operation on the same backend, and the same input
  to a backend must give the same topology and the same history. The rig checks this per backend:
  each case runs twice, and on two thread counts, and its records and histories must be
  byte-identical. The naming corpus already builds every design twice and compares. A backend
  whose output varies from run to run isn't routed for any operation.

### What can differ on another backend, and the answer

On solidcore nothing changes: the same functions run in the same order and make the same records,
so they produce the same histories and the same names.

| What differs | How it moves names | The answer |
|---|---|---|
| **Topology granularity:** a periodic face split in two, an extra vertex, coplanar faces merged differently | Two faces of `F2:side:l4` become pieces `#1`/`#2`; an extra edge adds a `~k` | `export` normalises to the record's conventions: one face per periodic surface with a seam edge, same-domain faces merged as solidcore merges them, no valence-2 vertex on a smooth edge. This is record-level work, and the history is composed through it. |
| **Blend corners:** a ball patch in one kernel, setback patches in another | The corner faces' `Edge` origins differ | A corner face's origins are the set of edges meeting there, so its name is set-based. A backend whose corner topology still differs fails the naming comparison for `Fillet` and isn't routed for it. |
| **Unverified names:** geometric fallbacks, and pieces numbered by place | They follow where faces lie | They are unstable by definition. The comparison (§8) reports them apart from verified names. Routing needs 100% of verified names to match. |
| **Face sources and certificates:** `FaceSource` (`Box`, `CertifiedExtrudeCap/Side`, `ShellPlanar`, `CylinderCap`, `Imported`) and the `with_*_certificate` calls | A certificate that assumes solidcore's face order would be wrong on another order | Certificates are carried by role through history, never by index (`BodyHistory::face_sources`). Each `with_*_certificate` call is audited in the pull request that routes its operation. |

### Which corpus designs are at risk

These counts come from grepping feature kinds over the corpus sources on `origin/main`:
`apps/solidcraft-cli/tests/corpus`, `examples/**`, `tests/action_matrix/designs`, the engine
fixtures, the hole, taper and projection designs, and `tests/oracle`. The generated samples
aren't counted, and one design can appear in several rows.

| Risk | Designs (files) | Why |
|---|---|---|
| Low | designs built only from primitives, extrudes, revolves and booleans | few faces, and fully exact histories on both sides |
| Medium | fillet 36, chamfer 15, offset 14, shell 13, sweep 7, loft 10 | blend corners, offset roles and sweep caps are where topology and roles can differ |
| High | sheet metal 4, and any design with an unverified name on the default backend | sheet names ride on rules built for solidcore's topology (`docs/design/sheet-metal-naming.md`), and unverified names move with geometry |
| None in v1 | STEP and IGES imports, mesh bodies, transforms, pattern copies | not routed in v1 |

The real list comes from the first cross-backend run (§8). It prints one row per design:
identical, verified names differ, only unverified names differ, or fails to build.

### Keeping 180/180 identical on solidcore

The default path must stay byte-identical: the same records, histories, names and saved models.
That holds by construction and is checked on every pull request.

**By construction:**

- **No solidcore operation changes** in this series. The router's solidcore arm is the existing
  call.
- **A solidcore-only build has one backend.** The router's `match` has one arm and folds away,
  and the `SOLIDCRAFT_KERNEL` read isn't compiled in (§9).
- **`Body`'s layout doesn't change** in a solidcore-only build. Its `foreign` slot is cfg-gated.
- **Nothing new is stored on the all-solidcore route.** Cache keys and `ErrorCode` stay as they
  are, and design files carry no `kernel` field when the route is all-solidcore (§5).

**Checked in every pull request of §12:**

- **The naming corpus**
  (`cargo test --profile ci -p solidcraft-cli --test naming_corpus`) is identical twice:
  - on the default build;
  - on a build with an alternative backend compiled in but `SOLIDCRAFT_KERNEL` unset, which
    proves that compiling a backend in changes nothing.
- **The kernel rig** gives a `results.json` identical byte for byte to main's (an empty `diff`),
  and the ladder's `golden.json` is unchanged.
- **Goldens and NLT** are unchanged, judged by their REGRESSION lines.
- **The loopback backend** (§12, step 12) must give 180/180 identical names. Loopback is
  solidcore forced through `export` → `import` and tag-and-read history on every operation. It
  proves the exchange and the renumbering exact before any foreign kernel is involved.

---

## 5. Designs record their kernel

A design opened on another backend must replay and report what changed. It must never quietly
come out different.

### What a design file stores

The feature history is unchanged: it holds no kernel ids, and any backend replays it.

When a design is saved under a route other than all-solidcore, it adds one field, `kernel`:

```json
"kernel": {
  "route": { "fillet": "alt", "boolean": "solidcore" },
  "versions": { "solidcore": "<version and source digest>", "alt": "<version and source digest>" },
  "names": { "F3": "<digest of the face and edge names F3's bodies had>", "F7": "…" }
}
```

- `route` lists only the operations that weren't on solidcore.
- `names` holds a digest of each feature's face and edge names as built. The full names are
  already in the saved model when the model cache is saved; the digest makes the comparison
  possible without it.
- A missing `kernel` field means all-solidcore. Every design saved today, and every design saved
  on the default route afterwards, has no such field. Their bytes don't change, so the corpus's
  "opened and saved again is the same bytes" check still holds.
- The field is a design-format change. It is one new file in `crates/doc/src/format/pending/`,
  in the pull request that adds routing (§12, step 15).

### Opening on a different route

When the running route differs from the one the design records, or from all-solidcore for a
design with no `kernel` field:

1. **The evaluation cache isn't reused.** Its key includes the route, so the design replays in
   full. This is the same path as opening a design saved without a cache.
2. **Names are compared.** After the replay, each feature's names are checked against the stored
   digest. Where a saved model exists, they are also checked against the saved names, face by
   face.
3. **A kernel differences report is shown.** It uses the same channel as an import report, and
   lists:
   - features whose names differ;
   - features whose face counts, volume or area moved past tolerance;
   - features that failed on this backend but built on the recorded one.
4. **References fail as they do today.** A stored name that no longer finds its face or edge
   fails its feature and asks for the reference to be picked again. Another kernel's face is
   never taken silently.
5. **The design is not rewritten.** Saving afterwards records the route it was saved under.

### Bug and crash reports record the kernel too

So triage knows which kernel made a body, every report carries the route and the backend
versions. This covers the in-app bug report, a crash report, the system block a ticket gets, and
`diagnostics.*` over MCP.

- **Always recorded, the default included.** The field reads, for example,
  `kernel: { route: "all solidcore", versions: { solidcore: "<version and source digest>" } }`.
  Reports aren't designs, so adding the field to them changes no design file's bytes.
- **The failing call says which backend.** A failed operation's `ErrorCode` already names it: a
  non-solidcore backend's `operation` starts with its adapter's crate path (§6). A report also
  names the backend that made each body the failing feature read, which the router knows.
- **Triage replays on solidcore first.** A report on a non-default route is replayed with
  `solidcraft-cli run replay.json --in <design>.sc --kernel solidcore`, then on the reported
  route. That separates the cases:
  - fails on both routes: an app or solidcore bug, triaged as today;
  - fails only on the reported route: a bug in that backend or its adapter, labelled with the
    backend and filed against its adapter. A kernel defect is reported upstream from there.
- **Design files already record the route** for non-default routes (above), so a design attached
  to a report carries it as well.

---

## 6. Errors

### The typed error every adapter returns

```rust
pub enum BackendError {
    /// The input is wrong whatever the kernel: not finite, out of range, an open profile, an
    /// index past its list. Only for input every backend must refuse.
    Invalid(String),
    /// This backend doesn't build this (yet). Never shown to a user while a fallback exists.
    Unsupported(String),
    /// A size too large for the geometry around it, with the largest size that builds where the
    /// kernel names one.
    TooLarge { why: String, largest: Option<f64> },
    /// No room: an inward offset leaves none, walls touch, a wall folds (largest where named).
    NoRoom { why: String, largest: Option<f64> },
    /// A numerical step failed: no convergence, a singular system.
    Numerical(String),
    /// The kernel made something, and it fails the shared check (§7).
    InvalidResult(String),
    /// Stopped by the cancel token.
    Cancelled,
    /// The kernel panicked (caught by `guard`).
    Internal(String),
    /// The conversion failed: a record the backend's form can't hold, or a form that doesn't
    /// export.
    Exchange(String),
}
```

### How each kernel maps into it

| `BackendError` | solidcore (`solidcore_ops::Error`) | alternative kernel (typical error kinds) | `KernelError` the app sees |
|---|---|---|---|
| `Invalid` | `Argument`; `Refused` only where the app's own checks agree the input is invalid | "invalid input", only after the same agreement check | `Invalid` |
| `Unsupported` | `Unsupported`, `CornerUnsupported`, `TurnRefused { room: false }` | "unsupported" | `Failed` ("not supported yet"), or fallback |
| `TooLarge` | `TooLarge { size, … }`, `RoundsCross { fit }` | where the kernel names a largest size | `TooLarge` |
| `NoRoom` | `NoRoom`, `WallsTouch`, `WallFolds { largest }` | where the kernel names it | `Failed`, with the same text as today |
| `Numerical` | `Status(_)`, `BlendNotCertified` | "numerical failure" | `Failed` |
| `InvalidResult` | `NotSolid`, `NotSolidBecause(_)`, `Empty` where the input was solid | "invalid result" with the checker's issues | `Failed` |
| `Internal` | `guard` | `guard` | `Internal` |

- **The solidcore row is today's mapping, moved, not changed.** The words a user sees on the
  default path stay the same.
- **`ErrorCode { call, kind, operation }` gains no field,** because a new field would change
  every stored report. Another backend's codes keep `call` as is (`"blend"`), take `kind` from
  that backend's own numbering, and start `operation` with that adapter's crate path. A
  solidcore code is byte-identical to today's, and two backends' codes never collide.

### A refusal of valid input is never allowed

The kernel rule (`.agents/skills/solidcraft-kernel-rules`) holds within each backend and across
them:

- **Fallback.** When a non-default backend answers a routed operation with `Unsupported`,
  `Numerical`, `InvalidResult`, or an `Invalid` that the agreement check doesn't confirm, the
  operation runs again on solidcore. The user gets solidcore's answer, and the miss is logged and
  counted (§8). This is `Fallback::ToDefault`, the mode a routed run uses.
- **Strict mode.** In the rig and in cross-backend runs (`Fallback::Strict`), the miss is the
  result: it counts as "refused" for that backend on that case.
- **Routing.** An operation is routed to a backend only when, on that operation's cases, the
  backend refuses nothing solidcore builds (§8). The fallback is a safety net, not the plan.
- **Agreement on `Invalid`.** A non-default backend's `Invalid` is shown as `Invalid` only when
  the app's own input checks, the ones that run before any kernel (`box_solid`'s size test, for
  example), refuse that input too. Otherwise it counts as `Unsupported`, and the operation falls
  back.

---

## 7. Checks

### One shared check

The check that decides whether a feature's bodies stand runs on the **record**. It is the same
check whatever made the body: today's `validity()` and `check_kept`, meaning `Brep::validate`,
`record_check::agreement`, and solidcore's own check of the record.

- **One judge makes the comparison fair.** A body only its own kernel's checker passes isn't yet
  a valid SolidCraft body. With one judge, "valid" means the same in every column of §8's table.
- **The default path doesn't change.** It runs the same check on the same record.
- **A foreign body costs one conversion to check.** It is checked through its solidcore form,
  built once and then kept in the `Body`'s cache. That cost is part of what §8 measures.

### Backend checkers are a diagnostic

An adapter may run its kernel's own checker:

- **It may refuse.** A result the backend's checker refuses becomes `InvalidResult` before export.
  That is cheap, and the operation falls back.
- **It never accepts.** Its verdict goes in a new field, `Validity::native`, set only for foreign
  bodies. The router treats that verdict as a refusal or as information, never as a pass.

The rig lists every disagreement as a finding:

- the shared check refuses and the native check passes: a backend bug, or a solidcore check that
  is too strict (a solidcore issue);
- the native check refuses and the shared check passes: a gap in the shared check.

### The per-feature background check (#21689)

#21689 lets the window show a feature's result before its check ends. If the check then refuses,
it flips the feature to failed through `Checks::refuse`, with the same error an evaluation that
waited would show. This design fits that unchanged:

- **The background check runs the same code whichever backend made the body.** It is the shared
  record check above.
- **Fallback happens inside the operation,** before the result is shown. The background check
  only sees results the router accepted. If it refuses a foreign body, the feature fails exactly
  as #21689 describes. There is no second, silent rebuild on solidcore: a rebuild nobody asked for
  would change results behind the user's back.
- **The report names the backend.** The router knows which backend made the body
  (`Validity::native` and the log), so the refusal says so, and the comparison counts it (§8).
- **Cached results key on the route** (§5). A design built on one route never loads results
  checked under another.

---

## 8. Selection, routing and measurement

### Selection

| Level | How | Default | When |
|---|---|---|---|
| Build | Cargo features on `solidcraft-kernel`: `backend-solidcore` (always on), one `backend-<name>` per other kernel whose authors bring an adapter, and `backend-loopback` (tests only). The apps forward them. | solidcore only | phase 1 has the solidcore feature only |
| Run, whole session | `SOLIDCRAFT_KERNEL=<name>` or `solidcraft-cli --kernel <name>`: every routable `OpKind` the named backend supports goes to it; the rest stay on solidcore | unset (solidcore) | **later**, not phase 1 |
| Run, per operation | `SOLIDCRAFT_KERNEL_ROUTE=fillet=<name>,boolean=solidcore`, or `--kernel-route route.toml`; this wins over `SOLIDCRAFT_KERNEL` | none | **later**, not phase 1 |
| Shipped route | `crates/kernel/route.toml`, compiled in: the per-operation choices measurement supports | all solidcore | **later**; changes are the owner's (§13) |
| Per design | the design's recorded route (§5), offered when the design opens on a build that has those backends | follow the running route | **future** |
| Preferences | a kernel choice in Preferences, among the compiled backends | solidcore | **future**, a UX decision |

These hold at every level:

- **Compile time decides what can be chosen** (owner decision, see Decisions). In phase 1 the
  router's choice is a constant: solidcore is the only kernel compiled in, so there is nothing to
  read at launch.
- **When run-time selection comes,** the route is read once, into a `OnceLock<Route>`. Switching
  takes a restart (§11).
- **A bad name never crashes the app.** An unknown backend, or one this build doesn't have, is
  logged once and ignored.
- **The route enters the evaluation cache key** only when it isn't all-solidcore. Existing caches
  stay valid.

### The kernel support matrix

`docs/kernel-support.md` (added with the router, §12) lists every kernel with an adapter to our
interface: who maintains its adapter, the commit it pins, each `OpKind` it supports, and its
results on the naming gate and the accuracy gate below. In phase 1 it has one row, solidcore,
maintained by us. A kernel that isn't in the matrix, or that fails either gate on an operation,
isn't routed for that operation.

### One comparison table

Three measures each gain a `--kernel` switch and strict mode, and one new xtask joins their
results:

1. **The kernel rig** (`cargo xtask kernel-cases --kernel <name>`) runs every case on that
   backend, in strict mode. `results.json` gains these fields per case:
   - `backend`, status, counts, volume and area;
   - time in the operation, and time in the exchange;
   - `ExchangeLoss`;
   - the shared check's verdict and the native check's verdict;
   - **accuracy**, all in millimetres at the case's own scale:
     - the largest edge and vertex tolerance the result record carries (the gap the kernel had to
       declare);
     - the largest gap the shared check measures between each edge's curve, its pcurves and its
       faces (`record_check::agreement`);
     - the relative error in volume and area against the case's analytic answer.

   The rig's answers come from analysis, never from a kernel (tests/kernel-cases/README.md), so
   they are already neutral.
2. **brep-bench** (bmander/brep-bench, Apache-2.0) gets a runner, `solidcraft-kernel --kernel
   <name>`. The runner builds each case through the router, converts the result to the record,
   and writes STEP with solidcore's STEP writer. That measures a backend as SolidCraft would use it, conversions included, graded by
   the bench's own closed-form answers.
3. **The naming corpus** gains `NAMING_CORPUS_KERNEL=<name>`. It builds each design on solidcore
   and on that backend. For each design it reports one of:
   - `identical`;
   - `verified names differ`, with the diff;
   - `only unverified names differ`;
   - `fails to build`, with the step and the error.

`cargo xtask kernel-compare out/solidcore out/<name> …` joins the runs into one table, written to
`target/kernel-compare/table.md` and `.json`. It has one row per `OpKind` and one column group
per backend:

| `OpKind` | cases | solidcore pass / refused / wrong / median ms / worst tolerance | `<name>` pass / refused / wrong / median ms / exchange ms / losses / worst tolerance | cases looser than solidcore | names identical (designs) |
|---|---|---|---|---|---|

Each row expands to its cases. A scheduled CI job (nightly, not per pull request) builds the
non-default features and publishes the table. Nothing in it gates the default build.

### The gates another kernel's adapter must pass

These apply only if someone brings another kernel; none is planned. The adapter is theirs to
write and maintain. An `OpKind` may go to another backend only when all six hold on its rows.
Rules 2 (the naming gate) and 5 (the accuracy gate) are the owner's hard requirements:

1. **No new refusals.** No case solidcore passes is refused or wrong on that backend (§6).
2. **Same names.** Verified names are identical on every corpus design that uses the operation.
3. **Shared check.** No result fails the shared check.
4. **No loss.** There is no `ExchangeLoss` on a case solidcore builds losslessly, unless the loss
   is a label only and every later case on that body still passes.
5. **As accurate (a hard gate).** On every case, the backend's result holds the same linear
   tolerance as solidcore's or a tighter one. Each of the three accuracy measures (the worst
   tolerance it carries, the worst gap the shared check measures, and the volume and area error
   against the analytic answer) is no larger than solidcore's on the same case. The comparison is
   made after the exchange, on the whole body, so a conversion that loosens faces the operation
   didn't touch counts against it.
   - Why it is hard: solidcore results typically hold about 1e-12 mm at part scale, while some
     kernels work to a single global tolerance near 1e-7. That is invisible on one part, but on a
     large assembly placed and combined many times over it compounds to millimetre-level error.
   - Speed never buys back accuracy. A faster backend that is looser on any case isn't routed.
     The "cases looser than solidcore" column must read 0.
6. **Clearly better.** End to end, conversions included, the backend is at least 1.2× faster at
   the median and never more than 1.5× slower on any case, or it passes cases solidcore refuses.

The decision is recorded on the operation's tracking issue, with the table. The shipped route
stays all-solidcore until the owner says otherwise (§13).

---

## 9. Performance and binary cost

### Dispatch: measured

The owner asked whether `dyn` or static dispatch at the app boundary costs the solidcore path
anything. A microbenchmark compared three ways of calling the same out-of-line operation:

- a direct call;
- a closed `enum` of backends, chosen at run time from a `OnceLock` route;
- `&'static dyn Backend` chosen at run time.

Setup: 200 million calls per round, three rounds, `rustc 1.100.0 -O -C codegen-units=1`, on an
x86-64 Linux build machine.

| Round | direct | enum | dyn |
|---|---|---|---|
| 0 | 3.30 ns | 2.39 ns | 2.39 ns |
| 1 | 2.39 ns | 2.59 ns | 2.39 ns |
| 2 | 2.39 ns | 2.59 ns | 2.39 ns |

The three are within 0.2 ns of each other, which is noise. The cheapest kernel operation the
router dispatches (a box) takes microseconds, and the common ones take milliseconds to seconds,
so **run-time selection costs nothing measurable either way.**

**The choice is a closed `enum`, not `dyn`:**

```rust
enum Which { Solidcore, #[cfg(feature = "backend-alt")] Alt, /* … */ }

pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    match route::of(OpKind::Fillet) {
        Which::Solidcore => blend::fillet(body, edges, radius), // today's code path, unchanged
        #[cfg(feature = "backend-alt")]
        Which::Alt => routed::<Alt>(OpKind::Fillet, body, |b, f, idx| b.fillet(f, idx, radius, FilletTurn::Mitre)),
    }
}
```

- **The solidcore-only build makes the same call as today.** With one variant, `route::of` is a
  `const fn`, the `match` disappears, and the call is the call it is today. That is zero
  overhead by construction, not just too small to measure.
- **A build with several backends still selects at run time.** The enum holds every compiled
  backend, and the route picks among them on each call. No rebuild is needed to switch.
- **`dyn` would buy nothing.** The trait has an associated `Form` type, so it isn't object-safe.
  Making it object-safe means boxing every form and erasing every signature, for no measured
  gain. And if a hot record service is routed later (`measure` has 375 references,
  `classify_point` over 200, called in loops), an enum arm can still be inlined.
- **`dyn` stays possible later.** If a backend ever has to load as a plug-in without a rebuild,
  it can be wrapped behind a boxed adapter that is one more enum variant (`Which::Plugin(Box<dyn
  DynBackend>)`). That is out of scope here.

### Conversion

| Cost | When | How it's kept down |
|---|---|---|
| Record → backend form | once per body per backend | cached on `Body` (the `foreign` slot), shared by clones, like the `smlib` cache |
| Backend form → record | once per result | needed anyway, for names, meshes and saving. Pcurve fitting is the expensive part; it reuses the record's own routine. |
| Back and forth between backends | each time the route alternates on one body | counted in the measured end-to-end time (§8); routes go per family (all blends on one backend), not per call |
| The shared check of a foreign body | once per kept body | the record → solidcore conversion it needs is cached, and its time is measured |

On the default path there's no conversion beyond today's.

### Several backends in one binary

- **What ships by default.** Release builds (desktop, CLI and web) compile **solidcore only**.
  Builds with other backends are developer and nightly measurement builds until the owner routes
  an operation to another backend by default.
- **Size.** Each backend adds its kernel's code and its dependencies. The adapter pull request
  measures it (release binary size and wasm size, before and after, plus `cargo bloat --crates`)
  and states it in its description. No number is guessed here.
- **Dependencies.** An alternative kernel brings its own dependency tree (linear-algebra crates
  and the like). Builds with the feature run `cargo deny` and `cargo xtask licences`. A
  dependency with a licence SolidCraft doesn't accept blocks the adapter.
- **wasm.** The application builds for wasm (AGENTS.md), so a backend can be routed by default
  only once its feature builds for wasm and passes `cargo xtask web-offline-test`.
- **Build time.** A feature that isn't selected costs nothing to build. The nightly comparison job
  builds each backend's feature.

---

## 10. solidcore only through ops, only through the adapter

The owner's rule: SolidCraft uses `solidcore-ops` fully, and **only** through the solidcore
adapter. This section is the inventory of what reaches into solidcore today, the plan for each
item, and how the rule is enforced.

### Dependency edges today (Cargo.toml, `origin/main`)

| SolidCraft crate | solidcore crates |
|---|---|
| `crates/kernel` | record, ops, tess, step |
| `crates/smlib` | **solidcore** (the kernel crate, via `xtask/solidcore-seam.txt`), record, ops |
| `crates/geom` | **solidcore** (seam), record, ops, tess |
| `crates/io` | record, ops, tess, step, cadir |
| `crates/doc` | record, ops, tess |
| `crates/engine` | record, ops |
| `crates/sketch` | record, ops |
| `crates/develop` | record, ops |
| `crates/ui-egui` | record, ops, tess |
| `crates/gpu`, `crates/render`, `crates/viewport`, `crates/viewport-gpu` | record, tess |
| `crates/dynamics`, `apps/solidcraft-cli` | record |
| `tools/bench/tess` (path dependency) | record, ops, tess |

`xtask/solidcore-public.txt` currently lets SolidCraft use `-ops`, `-tess`, `-record`, `-step`
(until RFC 0003 1.6) and `-cadir` (io only, until 1.6) directly. The seam list has two lines:
`crates/smlib -> solidcore` and `crates/geom -> solidcore`.

### Inventory below ops, with the plan for each

The classes are: data types, computation, tessellation, file formats, and validation. Record data
types (`Vec3`, `Vec2`, `Plane`, `Seg2`/`Loop2`/`Region2`, `brep::{Brep, Curve, Surface, Frame,
…}`, `history::*`, `part::*`) appear in about 2,700 lines across nearly every crate. They are
summarised under "May stay" below. Every other use is listed here.

#### `crates/smlib`: direct calls into the solidcore kernel (about 700 lines in 41 files)

These are the pre-ops wrapper. The seam line already says they go at "the switch commit of the
solidcore ops stage 3b plan". The plan for each is to route it through the ops family that
already exists on main. Where a needed entry point is missing, it is first added to
`solidcore-ops` in its own `solidcore/` pull request.

| smlib file:function | Kernel items used | Plan |
|---|---|---|
| `core.rs:run`, `answer` | `kernel::{Kernel, KernelOptions, CancelToken, UpstreamFixes}`, attribute store, `brep_volume`, `create_box/cone/cylinder/sphere/torus`, `SolidError` status | `ops::session`, `ops::cancel`, `ops::Body` primitives and volume (exist) |
| `core/boolean.rs:merged` | `boolean::api_brep::boolean`, `BooleanOperation`, `update_tolerances_after_boolean` | `ops::boolean` (exists) |
| `core/blend.rs` (incl. `finish`) | `edge_closest_point`, `brep_copy`, `validate_and_update_tolerances`, `MIXED_CORNER_RAILS_CROSS`, failure types | `ops::blend`, `ops::check`, `ops::error` (exist). Add `ops::pick` edge-closest if it is missing. |
| `core/convert.rs` | curve, NURBS, surface, attribute and extent types, bounding boxes, `make_topology_from_data`, `brep_data*` | `ops::Body::from_record` and `to_record` (exist; `solidcore-convert` is their implementation) |
| `core/file.rs` | `read_brep_from_file`, `write_brep_to_file` | `ops::file` (exists) |
| `core/history.rs` | attribute store, `edge_closest_point`, `brep_copy` | `ops::history`, `ops::provenance` (exist) |
| `core/measure.rs` (incl. `check_scoped`) | assert arrays, `context_xsect_tol_3d`, `GapFunction`, `brep_properties_in_box`, `face_properties`, `brep_bounding_box`, `brep_is_manifold_solid`, `brep_material_census`, `face_bounding_box`, `check_brep_scoped`, `PPF_AREA` | `ops::measure`, `ops::check`, `ops::query` (exist). Add `ops::measure` in-box properties if missing. |
| `core/merge.rs` | `is_manifold_solid`, `delete_face`, `orient_trimmed_surfaces`, `stitch_faces`, `remove_topological_edges_and_vertices` | `ops::stitch`, `ops::heal` (exist). Add face-delete and edge/vertex-removal recipes if missing. |
| `core/offset.rs` | `offset_brep_simple`, `local_operation_simple`, `OffsetFacesExecutive`, `shell_brep`, `face_classify_uv`, `face_closest_point_in_shape`, `face_internal_point`, `BrepQuery`, `PointClassType` | `ops::offset`, `ops::query` (exist) |
| `core/split.rs` | `boolean_with_options`, `MergeOptions`, `is_sheet`, `new_brep`, `create_face_from_surface`, `orient_sheet_surfaces`, `merge_curves_on_surface_through_surface`, `face_planar_section_curves`, `is_embedded`, `brep_cache` | `ops::split`, `ops::boolean` (exist) |
| `core/ssi.rs:surface`, `domain`, `evaluate`, `intersect` | `surface_intersector::do_intersection`, `SurfaceIntersection`, `TsectCurveType` | **Add** an app-agnostic surface/surface intersection entry point to `solidcore-ops` (solidcore pull request), then call it |
| `core/sweep.rs` | `create_linear_sweep`, `create_rotational_sweep`, `create_curve_sweep_with_placements`, `create_taper_extrude_with_corner`, `create_skin_primitive`, `compute_crvs_normal`, `api_stitch_into_solid`, `create_parallel_projection_curves`, `stitch_faces`, `stitch_and_orient`, `copy_faces`, `create_face(s)_*`, `create_planar_section_curves`, `context_zone_tol_3d` | `ops::sweep`, `ops::prism`, `ops::stitch` (exist). Add the tapered extrude with corner if missing. |
| `core/trim.rs` | `edge_closest_point`, `face_closest_point_in_shape`, `face_internal_point`, `Continuity`, `BoxSide`, `copy_source_ref`, `delete_face`, `new_brep`, `make_edge_in_face`, `copy_faces_from`, `create_face_from_surface`, `make_vertex_loop`, `get_or_create_uv_trim_curve`, `create_missing_uv_trim_curves` | `ops::trim`, `ops::untrim`, `ops::extend` (exist) |
| `core/transform.rs` | `mirror::transform_matrix` | `ops::affine` (exists) |
| `core/picked.rs`, `dump.rs` | `brep_copy`, `edge`, `vertex`, `Point3` | `ops::pick`, `ops::Body::copy` (exist) |
| `stop.rs`, `lib.rs` | `CancelToken`, `SolidError` (crash test) | `ops::cancel`, `ops::session` (exist) |
| `*_tests.rs` | `FilletSession`, `boolean_difference/merge`, `create_box/plane/rectangle/line_segment`, `trim_surface_with_3d_curves`, `create_linear_sweep`, `brep_area`, `merge_brep`, `new_brep`, `copy_faces`, `points_in_face` | tests move with their code into the adapter and call ops; the kernel-level ones move to solidcore's own tests |

#### `crates/geom`: the seam into `solidcore::eval::plan`

| geom file:function | Item | Plan |
|---|---|---|
| `bspline.rs` (top-level `pub use`) | `eval::plan::basis::{Side, basis_derivatives, binomial, local_basis, local_basis_array}` | **Add** `solidcore-ops` curve-evaluation entry points (B-spline point and derivatives at a parameter; the kernel computes, ops sequences) in a solidcore pull request. geom's B-spline evaluation then moves into the adapter, and its callers (sketch, doc) call it through `crates/kernel`. This is RFC 0003 item 1.8. |
| `bspline.rs:interpolate`, `bspline.rs:eval` | `eval::plan::{span_of, basis_ders}` | as above |
| `lib.rs` `mod tess` | `pub use solidcore::eval::plan::*` (`EvalCurve`, `EvalSurface`, `uv`, …) | the plan types are `solidcore-tess`'s display plan. Their users take them from the adapter's re-export, or, in the shared display crates, from `solidcore-tess` under the display exception (§13, question 2) |
| `lib.rs` `tess::partials`, `to_kernel`, `from_kernel(3)` | `MAX_PARTIAL_ORDER`, `orders`, `trig_derivative`, `Partials`, `solidcore::Vec3` | move into the adapter with the evaluator |

Through geom's `tess` re-export, these reach the plan evaluator: kernel `blend.rs:curve_derivative`,
`bridge.rs:station`, `splitface.rs` (`tess::uv::Starts`), and tests in kernel, gpu, doc and
engine. They are all solidcore-adapter code or tests, so they move with it.

#### Tessellation (`solidcore-tess`)

| file:function | Item | Plan |
|---|---|---|
| kernel `body.rs` (re-exports), `mesh_plan_untimed`, `export_mesh`; `lib.rs` `From<TessError>`; tests | `plan`, `mesh`, `MeshSpec`, `MeshTolerance`, `PlanStats`, `DISPLAY_ANGLE`, `MESH_MIN_ANGLE`, `TessError` | adapter code; it stays, inside `crates/kernel-solidcore` |
| io `kicad_in.rs:extrude_regions`, `add_surface_region`; test | `triangulate_union`, `triangulate_planar` | route through `crates/kernel` re-exports (the adapter's tess face) |
| io `lib.rs` test | `Unshaped`, `ShapeLimit` | the same |
| doc `eval.rs` (fields, `gpu_plan_at`, `refined_display`), `gpu_plan.rs:gpu_plan` | `TessPlan` type | take the type from `crates/kernel`'s re-export |
| ui-egui `display_lod.rs`, `viewport.rs:drawn_plan`, `wireframe_highlight_plan` | `TessPlan` type | the same |
| viewport `scene.rs`, `raster.rs` (test); viewport-gpu `lib.rs:face_surface`, `view.rs` (about 19 sites), `gpu_cull.rs`, `capture.rs`, tests; gpu tests | `Mesh`, `TessPlan`, `FacePlan`, `EvalCurve`, `EvalSurface`, `simplify::Level`, `PoleNode`, `PoleRule` | these are **shared crates**, used by other front ends and allowed solidcore by `docs/architecture.md`. They can't depend on SolidCraft's adapter. Proposal: they keep `solidcore-tess` as the **display-format exception** (display data, and the display-only plan evaluator that solidcore's layers.md §3 allows); see §13, question 2 |
| tools/bench `tess/src/main.rs:main` | `plan`, `MeshTolerance::{display_of, export_of}` | a tool, not app code: it goes through `crates/kernel`, or is listed as a tool exception |

#### File formats (`solidcore-step`, `-cadir`, the record's STEP and native modules)

| file:function | Item | Plan |
|---|---|---|
| kernel `step_in/*` (`p21.rs:parse`, `check_memory`, `set_max_bytes`; `mod.rs:body_item`, `with_checks`, `context_units`, `frame_matrix`, `placement_matrix`, `mapped_shapes`, `rep_bodies`, `step_orientation_errors`, `step_validate`, `with_workers`, `finish_checking`; `parallel.rs:smlib_part`, `solid_part`, `surface_part`; `tessellated.rs`) | `solidcore_step::{p21, read, topology, units, geom}`, `solidcore_record::step::protocol` | adapter code, and it stays in the adapter. To meet "ops fully", **add** a STEP read entry point (parse, assemble, build, heal) to `solidcore-ops` in a solidcore pull request, which is layers.md §2 "Importers are solidcore's", then call it |
| kernel `step_out.rs` (`product_header`, `export_body`, `step_export_products`, `step_export_bodies_with_face_ids`) | `solidcore_step::write::product::*` | **add** a STEP write entry point to `solidcore-ops`, then call it |
| kernel `iges_out.rs:mapped_domain`, `shell`, `iges_export_bodies` | `solidcore_step::geom::*`, `write::topology::*` | **add** an IGES write entry point to `solidcore-ops` (or the STEP write op takes a format), then call it |
| kernel `native.rs:encode_native_body`, `decode_native_body`; tests | `solidcore_record::native::{encode, decode, write_value, read_value, check, check_value}` | adapter code; it stays. The native encoding is the record's own serialisation. |
| io `native_cad.rs:step_producer`; `native_cad/tests.rs` | `solidcore_record::step::p21::parse` | through `crates/kernel` (a header-read call next to `step_import`) |
| io `native_cad.rs:traced_records`; `native_cad/cadir_out.rs:brep` | `solidcore_cadir::{read_bodies, write_body, Written}` | **add** a CADIR read and write entry point to `solidcore-ops` (RFC 0003 1.6), and call it through `crates/kernel` |
| io `native_cad/convert/step_route.rs:name_unnamed_products` | `solidcore_step::write::string` | through `crates/kernel` (a STEP string-escape helper re-exported by the adapter) |
| io `limits.rs:memory_short` and its test | `p21::{Error::NeedsMemory, PEAK_PER_BYTE}` | use `solidcraft_kernel::step_check_memory`, which already exists |

#### Validation

| file:function | Item | Plan |
|---|---|---|
| kernel `body.rs:with_history_counts`, `with_checked_history`, `with_rounds_of`; `blend.rs:over_body_alone`, `corner_history`; `history.rs:check`; test | record `History::check_counts` | may stay: a structural check on record data (counts and indices), which is the record's job (layers.md §1) |
| smlib `core/blend.rs:finish`, `core/boolean.rs:merged`, `core/measure.rs:check_scoped` | `validation::{validate_and_update_tolerances, update_tolerances_after_boolean, check_brep_scoped}` | `ops::check` and the ops families above (step 8 of §12) |
| smlib, doc, geom tests | `Brep::validate`, `check_curve2`, `Budget::input` | may stay: the record's structural validation |

#### Ops used outside `crates/kernel` and `crates/smlib` (bypasses of the boundary)

These already use ops, but not through the adapter. The plan for each crate:

- `crates/kernel` re-exports the items, in a `solidcraft_kernel::services` module grouped like
  ops' own modules;
- one mechanical pull request per crate switches its `use` lines and drops its `solidcore-ops`
  dependency.

Results don't change, because they are the same functions.

| Crate | file:function | ops items |
|---|---|---|
| develop | `lib.rs` (use), `lib.rs:develop`, `fold` | `develop::*`, `query::Query` |
| doc | `assembly.rs:shift`; `canvas.rs:frame_corners`; `eval.rs:square_face_level`, `tapered_reach`, `move_turn_line` | `affine::{pivot_displacement, picture_frame, span_along, same_sense}`, `SquareFaceLevel` |
| doc | `config_certificates.rs:find_free_form`; `drawing/fill.rs:fill_profiles` | `prism::{cubic_contains, counterclockwise_sweep}` |
| doc | `document.rs:is_separate`; `eval_cell_grid.rs:tools` | `cell_grid::*` |
| doc | `eval_frame_trim.rs`; `frame.rs:member_axes`; `frame_paths.rs`; `gusset.rs`, `gusset.rs:geometry_error` | `frame_trim`, `frame_member`, `prism::tapered_flange_ends`, `frame_gusset`, `Error::*` |
| doc | `eval_surface.rs:loft_blend`, `point_frames` | `sweep::railed::{open_section_turns, point_frames}`, `Error::Unsupported` |
| doc | `feature_geometry.rs:restored_thread_pick` | `point_distance` |
| doc | `project.rs:resolve_uncached`, `edge_section`, `exact_edges`, `projection_links`, `meet_at_shared_ends`, `outline_piece`, `link_of_piece`, `loop_contains`, `project_sketch_curve`, `exact_silhouette_traced` | `curve::*`, `conic_projection::*`, `spline_projection::*`, `outline::*`, `pcurve::eval`, `query::Query` |
| engine | `cmd/canvas_sketch.rs:to_sketch`; `cmd/component_link.rs:design_images`; `cmd/file.rs:usd_decals`, `urdf_decals`; `cmd/sketch_canvas.rs:corners_in_sketch` | `affine::*` |
| engine | `cmd/hole_stacks.rs:hole_stacks`; `cmd/inspect.rs:classify_points` | `hole_stacks::*`, `ClassifiedPoint`, `PointNormalFace` |
| engine | `cmd/sketch_freeform.rs:project`, `recorded`, `support_hits`, `record_crossings`; `cmd/sketch_import.rs:insert_airfoil`; `cmd/sketch_rational.rs:interval` | `curve::*`, `sweep::Curve::from_seg2`, `curve_fit::fit_spline_bounds` |
| engine | `drawing_scene/fill.rs:dot_item`; `drawing_view_hatch.rs`; `drawing_scene/sketch.rs:check_arc_sweep` | `profile::*`, `prism::counterclockwise_sweep` |
| engine | `plugin/form.rs:pick_exists` | `Body::from_record`; use a `solidcraft_kernel::Body` method instead |
| geom | `brep/eval.rs:face_flux`; `brep/placement.rs:frame`, `surface`, `revolution`; `hlr.rs`, `section.rs`; `network.rs:network_surface` | `pcurve::*`, `affine::*`, `hlr::*`, `measure::points_meet`. geom is L0, below the adapter, so these functions **move up into the adapter**; their callers call them through `crates/kernel` |
| io | `airfoil.rs:normalized_points`; `dxf.rs:layered_ellipse`; `dxf/text_entity.rs:read_text_entity`; `dxf_out.rs:admit_bulged`, `sample_bulged_edge`, `shape_preflight`, `shape_emission_preflight`, `extend_extents`; `dxf_out/file_model*.rs` (`transformed`, `bounds`, `export_error`, `union`, `supporting_circle`, `point_bounds`, `ellipse_bounds`, `transformed_point`, `transformed_shape`); `dxf_out/path.rs`; `native_cad/cadir_textures.rs:decal_quad`; `urdf.rs:decal_dae`; `svg.rs:cubic` | `dxf::import::*`, `dxf::export::*`, `affine::{chord_coordinates, lifted_quad}`, `curve::cubic_pieces` |
| sketch | `cancel.rs`; `curves.rs:fit_spans`, `fit_beziers`, `fit_beziers_mode`; `link.rs:update_link_read`; `link_rebuild.rs:named_curve_identities`; `nurbs.rs:from_fit_points`, `check`, `homogeneous`, `reversed`, `linear_image`; `periodic.rs:validate_periodic_points`, `periodic_beziers`; `solver.rs:freeform_dist` | `cancel::CancelToken`, `fit_projection::*`, `point_identity::*`, `curve::closest_point`. sketch then depends on `crates/kernel` (both are L1, and there is no cycle) |
| ui-egui | `dialogs_assembly.rs:world_preview`, `placed_snap`; `dialogs_gusset.rs:for_feature`; `navigation.rs`; `pencil.rs:filter`; `ref_images.rs:show` | `affine::*`, `frame_gusset::support_pick`, `point_displacement`, `point_distance`, `point_set_summary` |
| tools/bench | `tess/src/main.rs` | `face_surface::FaceSurface` |

#### What may stay outside the adapter

- **The record's data types,** in any crate: `Vec3`, `Vec2`, `Plane`, `Seg2`/`Loop2`/`Region2`,
  `brep::{Brep, Curve, Surface, Frame, Edge, Loop, …}`, `history::*` and `part::*`, with their
  serde and their structural validation (`validate`, `check_counts`, `Budget`).
  - They are the body format itself, the way a file format crate is.
  - `solidcore-record` forbids maths (its `lib.rs` bans sqrt, trig and solves), so using it
    computes nothing below ops.
  - Moving about 2,700 lines to a SolidCraft copy would make a second implementation of one job.
  - Not allowed outside the adapter: the record's file-format modules, `solidcore_record::step`
    (the STEP parser) and `solidcore_record::native` (encode and decode). Those are import and
    export, so they belong in the adapter.
- **The shared display crates' `solidcore-tess` display types,** if the owner agrees (§13,
  question 2).

### End state and enforcement

- **Only `crates/kernel-solidcore` depends on** `solidcore-ops`, `solidcore-tess`,
  `solidcore-step` and `solidcore-cadir`. It reaches no crate below ops, so
  `xtask/solidcore-seam.txt` is empty and stays empty.
- **`solidcore-record` is allowed** in any crate, for its data types.
- **`xtask/solidcore-public.txt` becomes a list per crate:**

  ```text
  # crate                      may depend on
  crates/kernel-solidcore      solidcore-ops solidcore-tess solidcore-step solidcore-cadir solidcore-record
  *                            solidcore-record
  crates/viewport              solidcore-tess   # display-format exception, if approved
  crates/viewport-gpu          solidcore-tess   # display-format exception, if approved
  ```

- **`cargo xtask layers` checks two things:**
  - every crate's `Cargo.toml` dependencies against that list;
  - item paths: no `solidcore_record::step`, `solidcore_record::native` or `solidcore_ops::` path
    outside `crates/kernel-solidcore`.

  It is the same mechanism that kept truck inside `crates/kernel`. The list never grows; each
  migration pull request removes the line it makes unnecessary.

---

## 11. Switching kernels with a design open

| | v1 | Future |
|---|---|---|
| How | change `SOLIDCRAFT_KERNEL`/`SOLIDCRAFT_KERNEL_ROUTE`, or the CLI flag, and **restart** (owner decision: no hot swap in v1) | a Preferences or per-design choice, applied without restarting (hot swap, not v1) |
| What's required | nothing beyond §5: on reopening, the design replays under the new route and shows the kernel differences report | the route becomes a session value instead of a `OnceLock`; the evaluation cache is dropped (its key changes); a **full replay** from the first feature runs under the new route; names are compared and the report is shown; selection is re-resolved by name, as after any rebuild |
| Undo | not involved | the switch is a session setting, not a document change. It adds no undo step and doesn't mark the design modified. Saving records the new route (§5). |
| Bodies in memory | discarded by the restart | discarded. A body's cached forms are per backend, so nothing needs invalidating beyond the evaluation cache; the replay makes new bodies |

A partial replay (only the features whose operations changed backend, and everything after them)
is a later optimisation. A full replay is simpler and is always correct.

---

## 12. Migration

Rules for every step:

- one pull request, landable alone; a series is stacked into `main`, bases first;
- no change to the default path;
- each one shows the checks from §4: an identical naming corpus, an identical rig `results.json`,
  the ladder golden unchanged, and NLT with no REGRESSION lines.

The steps that move existing call sites don't change any function that runs.

### Phase 1: the seam (approved, built)

1. **This document**, docs only.
2. **`crates/kernel-api`.** The `Backend` trait, `OpKind`, `Support`, `BackendError` and the
   neutral input types whose definitions are plain data (`BoolOp`, `FilletTurn`, `ChamferSide`,
   the primitives and profile extents). `crates/kernel` re-exports the moved types under their old
   paths and maps `KernelError` to and from `BackendError`. It also adds the architecture table
   row and the `xtask layers` row. Nothing calls the trait yet.
3. **The solidcore adapter and the router.** `crates/kernel` implements `Backend for Solidcore`
   by delegating to its existing functions, and its public functions for each `OpKind` become the
   router: a one-variant choice, fixed at compile time, whose solidcore arm is the existing call.
   It adds `docs/kernel-support.md` with solidcore's row. It moves no code and changes no result.
   With it, `crates/kernel` re-exports the `solidcore-ops` and `solidcore-tess` items the other
   crates use (§10), grouped like ops' own modules, so every call site can move without the
   interface changing again.
4. **The bypasses, by area, in parallel.** One pull request per area, with no file shared between
   areas (engine commands, sketch, sheet metal, assemblies and joints, drawings, import and export
   glue, ui-egui, the MCP and CLI front ends, and so on). Each switches its `use` lines to
   `crates/kernel` and, once a crate has no direct use left, drops its `solidcore-ops` and
   `solidcore-tess` dependencies. Each is the same functions under new paths.
5. **The check, last.** `cargo xtask layers` fails when a SolidCraft crate other than the adapter
   (`crates/kernel`, `crates/smlib`) depends on a solidcore crate other than `solidcore-record`,
   apart from the listed exceptions of the later phases (geom, the shared display crates), each
   with its reason, in a list that only shrinks.

Phase 1 ends when every crate but the adapter, geom (step 6) and the shared display crates
(§13, question 2) reaches solidcore only through `crates/kernel`.

### Later phases (approved 2026-11-26)

These finish "solidcore only through ops" and make the seam usable by another kernel's adapter.
The owner approved them on 2026-11-26. Each step follows the rules at the top of this section.

6. **geom's ops and seam uses move up.**
   - Before any move, the missing `solidcore-ops` curve-evaluation entry points go in as their
     own solidcore pull request, which is RFC 0003 item 1.8.
   - Then one pull request per family moves geom's functions (placement, pcurve flux, hlr and
     section, network, the B-spline basis) into `crates/kernel`, and their callers follow.
   - The `crates/geom -> solidcore` seam line goes in the last one.
7. **io's file-format uses.** STEP stays in solidcore.
   - First the solidcore pull requests that add the missing ops entry points: STEP read and
     write, IGES write, CADIR read and write.
   - Then one io pull request switches its `solidcore-step`, `-cadir` and record STEP uses to
     `crates/kernel`.
   - The `solidcore-cadir` line leaves the public list.
8. **smlib below ops.** This is the existing stage 3b switch, one ops family per pull request
   (boolean, blend, offset, sweep, split, trim, measure and check, merge, convert and file,
   history, ssi, transform). Each family's missing ops entry points go to solidcore first. The
   `crates/smlib -> solidcore` seam line goes in the last one.
9. **The kernel's own STEP and IGES code** switches to the new ops entry points. The
   `solidcore-step` line leaves the public list.
10. **The split.**
    - `git mv` the solidcore code of `crates/kernel`, together with all of `crates/smlib`, into
      `crates/kernel-solidcore`, leaving `crates/kernel` as the router with the same public API.
    - `xtask layers` switches to the end-state rule (§10). From here, only the adapter depends on
      solidcore crates.
    - This is the only large diff in the series, and it is purely mechanical: moves and paths,
      with the same functions.
11. **Sweep and loft in the trait.** `PathSeg` and `SweepControl` carry geometry of their own
    (lengths, tangents), so they join `kernel-api` once that geometry is solidcore's.
12. **Exchange and loopback.**
    - The cfg-gated `foreign` slot, `export`/`import`, and history renumbering in the router.
    - `backend-loopback`, which forces solidcore through record → form → record and tag-and-read
      history on every operation.
    - A scheduled CI job runs the naming corpus and the rig under loopback; both must be
      identical to the default. Any difference is a bug in the exchange path, fixed here.
13. **Measurement.** `--kernel` and strict mode on the rig, `NAMING_CORPUS_KERNEL`,
    `xtask kernel-compare`, and the brep-bench runner. The loopback table is the first table, and
    every row in it equals solidcore's.
14. **Run-time selection.** `SOLIDCRAFT_KERNEL`, `SOLIDCRAFT_KERNEL_ROUTE`, the route file, the
    fallback policy, and the route in the cache key. The shipped route stays all-solidcore.
15. **Designs record their kernel.** The `kernel` field (one pending format file), the name
    digests, the replay comparison and the kernel differences report (§5).
16. **Future: run-time UI.** A per-design choice and a Preferences choice, and switching without
    a restart (§11). These are UX decisions for the owner.

**Another kernel's adapter is not a step of this plan.** If someone brings one, they write it to
the interface, behind a feature that is off by default, with its licence review (§13), its
size and dependency report (§9), and its rows in the support matrix. It is routed for an
operation only when it passes the gates in §8, and moving the shipped route off solidcore is
always the owner's decision.

Steps 2 to 10 stand on their own, even if no other kernel ever comes. They finish "solidcore only
through ops", which makes `solidcore-ops` a complete, documented API.

---

## 13. Risks and open questions

### Risks

- **Topology differences.** Two exact kernels can make different topologies that are both valid:
  corner patches, seams, split periodic faces. Normalising in `export` and the "verified names
  identical" rule (§8) contain this. Blends are likely the last family another backend wins.
- **Lost blend definitions.** A body that passes through a kernel without blend definitions comes
  back without them, and later solidcore blends can't re-certify those faces. The rig's chained
  blend cases measure this, and rule 4 of §8 blocks it.
- **Tolerant bodies.** Imports with loose tolerances stay on solidcore (`Support::No`) until a
  backend can hold local tolerances.
- **Mixed routes.** Switching kernels for alternate operations converts the body at every switch.
  The end-to-end timing in §8 decides, and routes go by family.
- **Kernels that move fast.** Each adapter pins a commit. The kernel's version and source digest
  go into cache keys and design files, so upgrading rebuilds rather than mixing results.
- **wasm.** No backend is routed by default until its feature builds and runs for wasm.
- **Cancellation.** solidcore honours SolidCraft's stop token inside a call; an alternative kernel
  may not. Long operations stay on solidcore until a backend can be stopped.
- **The size of §10.** Steps 4 to 9 touch many crates. Each pull request is mechanical and
  checked byte-identical, but together they are the bulk of the series.
- **Accuracy.** A backend that works to a looser global tolerance can look correct on single parts
  and still drift by millimetres across a large assembly. Rule 5 of §8 is a hard gate on every
  case, with no trade for speed, and the tolerance column keeps the gap visible.

### Tradeoff: support cost grows with every kernel

Every backend that can be built multiplies what support has to cover. This is an explicit cost to
weigh against any speed win.

- **Triage.** A report now has a route as well as a version. The triage matrix grows with the
  number of backends, and with every combination of per-operation routes in use. Replaying on
  solidcore separates the kernels (§5), but it is extra work on every report from a non-default
  route.
- **Upstream defects.** Bugs in another kernel are fixed upstream, on its maintainers' schedule.
  Until they are, the operation falls back or is un-routed, and someone has to own reporting it
  and tracking the fix.
- **Upgrades.** Each pinned kernel needs upgrading, its adapter re-measured (the full comparison
  table), and cached results rebuilt.
- **Documentation and tests.** Each backend needs its rig, corpus and bench runs kept green, plus
  its own known-failing lists.

How this design limits the cost:

- release builds ship solidcore only, so ordinary users report against one kernel until the owner
  routes an operation elsewhere by default;
- each backend routed by default needs a named owner for its adapter and for upstream liaison;
- a backend that wins nothing in the table after an agreed period is removed, adapter and feature
  together, rather than kept compiling.

### Open questions for the owner

Only questions 1 and 2 touch phase 1, and phase 1 goes ahead on the leaning given for each. The
others matter only if someone brings another kernel.

1. **The record as the neutral format, allowed everywhere.** `solidcore-record` is solidcore's
   data crate, not its kernel. The recommendation is to use it as the exchange format and allow
   its data types in every crate (§10): it computes nothing, and a SolidCraft copy would be a
   second implementation. Is that within "only the adapter depends on solidcore crates", or
   should every crate take the record types through `kernel-api` re-exports? That would be about
   2,700 mechanical line changes.

   **Another option to weigh: CADIR as the exchange format.** cadmpeg's vendor-neutral CADIR
   could be the exchange format instead of the record, validated by round trips on a large
   corpus. This is an option for the owner to weigh. The design as written uses the record, and
   it doesn't change because of this option.

   - **What CADIR carries.** It holds exact geometry on STEP's topology conventions: analytic and
     NURBS carriers, in millimetres and radians, with per-vertex and per-edge tolerances and a
     document linear tolerance. Bodies carry a name, and entities carry ids. SolidCraft already
     reads and writes it (`solidcore-cadir`, used by `crates/io`).
   - **What a record → CADIR → record round trip loses,** per `solidcore-cadir`'s own
     documentation:
     - procedural surfaces (blends, ball patches, placed revolutions) go as their NURBS nets,
       with no definition and no certificate (a blend's bound ε is written as the face's
       tolerance, which isn't read back);
     - pcurves aren't read back; the reader makes them again on its own parameters;
     - a record holding several regions comes back as one record per region, and wire edges are
       dropped;
     - CADIR has no field for persistent names or the operation `History` the naming rule needs
       (§4), so they would travel beside it in any case.
   - **What a round trip costs.** Each way is a full write and a full read, and the read is the
     STEP-style assembly: pcurves, seams, tolerances, canonical order. The record needs none of
     that between two operations: the solidcore backend uses it as it stands, with zero
     conversions.
   - **For CADIR:**
     - it is vendor-neutral and documented outside solidcore;
     - other tools already read and write it;
     - a round-trip corpus would test every backend's exchange against one independent format.
   - **Against CADIR:**
     - the losses above (blend definitions and certificates, pcurves, regions);
     - names and history would travel separately;
     - two conversions per operation, where the default path has none today;
     - the default path would no longer be byte-identical.
   - **For the record:** it is exact, it keeps blend definitions and certificates, it costs
     nothing on the default path, and names, meshes and the model cache already use it.
   - **Against the record:** it is solidcore's data crate, so the trait is tied to its schema.
   - **Which to use is the owner's call.** The design as written uses the record.
2. **The shared display crates.** viewport, viewport-gpu and gpu are shared with other front ends
   and use `solidcore-tess` display types: `TessPlan`, `Mesh`, and the display-only plan
   evaluator. Keep them as a listed display-format exception, or route them through a
   display-data crate? Phase 1 keeps them as the listed exception, and changes nothing there.
3. **Licence review for third-party kernels.** Each kernel gets a licence and provenance review
   before its adapter, covering its test kits and generated test suites. SolidCraft takes no
   GPL/LGPL code and doesn't read it, and a test kit derived from an LGPL kernel's suites
   disqualifies that kernel or that part of it. Who signs off on the review?
4. **Per-operation routing and "one implementation per job".** solidcore's rule is one
   implementation per job, chosen by measurement. Routing an operation elsewhere in shipped builds
   runs a second implementation in production. When another backend wins, should we:
   - (a) route to it;
   - (b) take the idea into solidcore, clean-room and within its licence, and stay all-solidcore;
   - (c) route first, then do (b)?

   The recommendation is (c).
5. **Who changes the shipped route?** Moving an `OpKind` off solidcore in default builds changes
   kernel results. Under the defaults rule that's a "no" (it isn't UX), but it is a product
   choice. Is it yours each time, or ours once the table clears §8's bar?
6. **Showing the kernel.** Should the History timeline show which kernel built a feature? The
   recommendation is not for now: only in logs, the kernel differences report and
   `diagnostics.*` over MCP. Anything visible is a UX decision.
7. **The run-time choice in the app.** The per-design and Preferences choices are marked as future
   work (§8, §11). When should they come, and which should come first?
8. **How another kernel would come in.** Decided in part: whoever brings it writes and maintains
   its adapter, behind an off-by-default feature, and it must pass the naming and accuracy gates
   and appear in the support matrix (§8). Still open: a git dependency pinned by commit, or
   vendored? The recommendation is a pinned git dependency, so the default build and `cargo deny`
   see nothing new.
