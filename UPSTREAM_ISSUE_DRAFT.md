# Combine Join refuses a flush contact after body-scaled coincident-face separation

## What happens

Combine → Join of two solids that share a face (they touch, they do not overlap) can fail with the boolean volume guard. A frame and a rail that share the plane y = −0.2, and also share a same-direction cap at z = −5, produce:

> the join could not be computed reliably: the joined body's volume would be 175946.7289 mm³ (the target is 159199.1580 mm³, the tool 16569.6000 mm³). The result was refused rather than kept wrong; where the bodies only touch (faces flush with each other), overlapping them slightly usually helps (temporary coincident-face separation up to 8.413626e0 mm per face)

Moving the whole rail 0.5 mm into the frame and joining again succeeds, and the diagnostics then name strategy `coplanar_separation`. That rigid move also shifts the rail's far face. The contact area is about 1705 mm², so the joined solid is about 851 mm³ short of the flush union (the two volumes added).

The parenthetical is the separation the kernel already applied. `face_shift_mm` for `BooleanStrategy::CoplanarSeparation` is `size * 0.0229` (or `size * 0.0137` on the grown path). On a body about 367 mm across that is 8.41 mm. Repeating that same shift cannot clear the guard: it is the attempt the guard refused.

## Where it is decided

Join is `solid.combine` with `FeatureKind::Combine`. Evaluation of `Operation::Join` is `apply_traced` in `crates/doc/src/eval.rs`, which calls `kernel::boolean(target, tool, BoolOp::Union)`. A mirror that combines the copy joins the same way.

`boolean_via` in `crates/kernel/src/ops.rs` runs the union, and when the attempt recorded a retry it calls `boolean_guard::check_retry` (`crates/kernel/src/boolean_guard.rs`). The strict slack is `2e-4 * (|va| + |vb|) + 1e-9`. For these volumes that is about 35 mm³. The result is about 178 mm³ above the sum (about 0.1%), so the guard refuses and `boolean_via` appends `BooleanDiagnostics::summary()`. The coplanar line of that summary is built in `crates/kernel/src/boolean_diagnostics.rs` (`temporary coincident-face separation up to {face_shift_mm:.6e} mm per face`). The sentence "overlapping them slightly usually helps" is the fixed text in `boolean_guard::bad`.

The separation itself is `coplanar::boolean_apart` (`crates/kernel/src/coplanar.rs`). Coincident planes are pushed by a depth scaled to the whole body (`size * 0.0229`, then a quarter and a sixteenth of that). Back-to-back faces are pushed into the other body. Same-direction faces are pushed outward and the slab is cut back off. An earlier, looser check in `boolean_whole` uses slack `2e-3 * (va + vb)`, so this heavy result is accepted there and only the strict guard refuses it.

A plain wall plus the same rail joins exactly, with an empty strategy list. The refusal shows up when the back-to-back contact is accompanied by another coincident situation the scaled push handles badly: here a same-direction cap at z = −5 and a hole through the wall. That is the same family as #5936 (a flush join refused when blind holes sit under the face), #5353 and #5413.

## Scope relative to the kernel pause

#3992 pauses changes under `crates/kernel/` while solidcore is evaluated. Correcting the body-scaled separation so the boolean itself returns the flush volume is that work. This report is the document-level retry around the refusal, using public kernel calls only (`boolean`, `offset_faces`, `faces`, `measure`, `validity`).

## Retry that matches the volume

On this refusal only, overlap the tool's back-to-back contact faces a fraction of a millimetre outward into the target, then join again. Keep the result only when `validity()` is empty and the measured volume is within the guard slack of the original target volume plus the original tool volume. Depths 0.05 mm, 0.2 mm and 0.5 mm sit far above the coincidence tolerance (about `size * 1e-6`) and far below the body-scaled shift. The added slab lies inside the target, so the outer shape is the flush join and the rail's far face stays at y = 9.8.

On the frame and rail, offsetting only the y = −0.2 contact by +0.5 mm leaves the outer rail face at y = 9.8 and the joined volume about 1 mm³ from the sum. The same holds for +0.2 mm. A rigid move of 0.5 mm does not: it loses the contact area times the move.

`contains()` is the wrong gate for "is this point inside". Its mesh tolerance is about `size * 2e-3` (about 0.7 mm on this frame), coarser than the overlap.

## Public reproduction

No design file is required.

- Wall from (−0.2, −10, −5) to (365, −0.2, 30). Cut a hole of radius 3 mm along +Y, base (340, −11, 5), length 12.
- Rail extruded from the pentagon (−0.2, −0.2), (9.8, 9.8), (345, 9.8), (350, 4.8), (355, −0.2), from z = −5 to z = −0.2. Volume 16569.6 mm³ (profile area 3452 mm² × 4.8 mm).
- `boolean` union of the wall and the rail returns the volume-guard error and names coincident-face separation (the recorded shift is about 8.40 mm on this smaller body).
- Offsetting the rail face at y = −0.2 outward by 0.05–0.5 mm and joining again yields one valid lump whose volume matches the sum and whose maximum y stays 9.8.

The wall without the hole joins directly.
