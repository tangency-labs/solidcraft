# Addendum 1: where the read path's seconds went — the memo never crossed a kernel

Addendum to `blend.fillet_section--free-support-certificate-cost-18324.md` (same day, same
design, same claim: tangency-labs/solidcraft#18324, PR #22347).

## What the first record got wrong, and what replaced it

- "The enclosure over-estimates and the subdivision compensates": the spans' final bounds
  close at a median 0.3 of the target, with no infinite bounds. What actually runs is a
  Krawczyk ladder: the contraction `q0` is first order in the piece box (measured 4.88,
  2.44, 1.22, 0.609 — halving per level), so pieces refine until existence proves. The
  Taylor models already carry the first order exactly; there is no over-estimation to
  remove, and the level count is the geometry's, not the arithmetic's.
- "Refine the support's knot spans": falsified with an exact-verified Boehm insertion
  (a unit test checks the refined form evaluates the same to 4e-16). The certificate's
  per-span Taylor model is sharp at any span size; refinement only adds knot lines to
  cross. The measured counts did not drop.
- The profiler's own scopes cost ~175 µs each inside the hot loop and inflated the
  enclosure's profiled cost about threefold (2.1 ms profiled, 0.78 ms by direct
  micro-timing). Fine-grained profiles of this path need manual timing, not more scopes.
- Micro-timing of one enclosure call: 84 µs of series setup, 374 µs of entry (the system
  model, its Jacobian, the preconditioner), 324 µs of the one tube iteration. The
  station solves are negligible (15 in the whole design's construction).

## The read path: two 4.5 s certificates for one key

The design's second big cost was a read that re-certified a record an identical memo key
already held. The memo state at each lookup named the mechanism: the lookups spread over
four kernels (a boolean's copy, the pools' kernels), and each cold kernel re-derives —
`cache_kernel` gave every copy a memo of its own, and the two kernel pools (the wrapper's
and solidcore-ops' session's) never shared one either.

## The fix

`Kernel::recertify_memo` is now an `Arc<Mutex<RecertifyMemo>>` that a boolean's copies
share with their parent (as `blend_operation_scopes` already did), and the wrapper's
`start()` hands one memo to both pools' kernel factories. A hit returns what
recertifying the same blend again returns — the memo is keyed by the tolerances, the
fixes and the whole record — so sharing changes no answer, only which kernel does the
work. On the design of #18324 the full chain goes 20.8-21.4 s → 16.2-16.7 s (cache
dropped, median of five; the volume, area, face and edge counts bit for bit identical),
and `blends_recertified` drops by the two re-derivations.

A kernel's copies can re-derive a slightly different valid bound than their parent (the
bound is a proof artifact: any achieved ε that proves the net within it is right). With
the memo shared, the parent's bound is the one kept, so 69 golden cases across five blend
units (of ~14,000 blend cases; every boolean unit unchanged) move within their
tolerance noise and are blessed with this record as their justification. The geometry of
the reported design is unchanged bit for bit.

## What is still open

The certificate's remaining ~9 s on this design is the first certification of the round
against the arc-swept wall (the construction's spans and the first read), at ~0.7 ms per
enclosure call and a piece size the Krawczyk test forces on that support's Jacobian.
The counts are in the first record; the levers there (the per-call work, the target, the
support's own closed forms) each need their own design decision.
