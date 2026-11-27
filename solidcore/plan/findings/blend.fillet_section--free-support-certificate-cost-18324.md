# The blend certificate's cost against a free (arc-swept) support: counts on #18324's flanged pipe

Unit: blend.fillet_section (the certificate `gn_certify_span`, `gn_enclose_tm`, `feet_tm`).
Found: 2026-11-26, from the claim of tangency-labs/solidcraft#18324 (PR #22347), measuring the
issue's 18-feature flanged pipe (a Ø40 pipe swept line-arc-line, shelled, a collar revolve, a
plate join, four rounds) on main `5ba869e6ae`, macOS arm64, release build, cache dropped.

## The counts (feature work ≈ 21 s of the 26 s full chain)

| where | scope | time | counts |
|---|---|---|---|
| Fillet1's construction | `blend::solver::trace > gn_certify_span` | 5.3 s | 263 spans, 1,572 `gn_enclose_tm` calls |
| the reads (adopt at `brep::to_kernel`) | `recertify > gn_certify_span` | 5.5 s | 194 spans, 1,884 `gn_enclose_tm` calls |
| the whole design | `gn_enclose_tm` + `gn_enclose_over` | ≈ 8 s + 1.7 s | |
| station solving | `blend` root's counters | negligible | 15 solves, 20 Newton steps, 32 evaluations |

Each span's certificate chases `target = 4e-6` mm (a tenth of the face's zone tolerance; the
kernel default `zone_tol_3d` is 1e-5). Per span the level loop runs a median of 4 levels and
**329 of 651 spans exhaust all four** (`SPAN_MAX_LEVEL`): the enclosure over-estimates, and the
subdivision compensates. The spans end a median 0.88 mm apart (minimum 8 µm) on a 188 mm
circle.

## The controls

- The same r=4 round on a plain cylinder (an exact class, docs/generic-blend.md §4): 17 ms, no
  certificate at all — no definition, nothing to enclose.
- A circle swept along a line (an analytic surface in the sweep's exact-class routing): 17 ms.
- A circle swept along a single arc is, by design (docs/procedural-sweep.md §4), "a B-spline
  carrying its definition" — its wall is a free NURBS to the blend, and the round against it
  pays the certificate: the cost is the free support, not the sweep or the shell.
- Size scaling: none (the same round at 1×, 10× and 100× part size costs the same).

## The mechanism

The certificate encloses the support's partials as Taylor models about the box middle
(`nurbs_partials_tm`: basis derivatives at the middle, the polynomial part exact, then the
rational projection through the weight's TM). The support is already enclosed piece by piece
(`SpanAt::ku` are knot-span indices), so the over-estimation is in the arithmetic of the
rational form, most plausibly the projective division and the composition into the section
rows. Which term dominates is not yet measured; a per-term bound dump is the next probe.

The read path re-derives because the memo key is the whole record's content: the plate join's
boolean re-trims the first round (its net changes), a legitimate miss, and the re-trimmed
(cut) net is re-certified from scratch although docs/generic-blend.md §2.4 proves cut nets as
pieces of the same Hermite net. The validation flag changes nothing on main (the keys are
identical with `--validation off`); an earlier hit-without-validation observation belonged to
the pre-#21966 binary, where the join did not re-trim the round.

## The literature the docs prescribe (generic-blend.md §2.4: "a tighter enclosure, never a
sampled check")

- Makino, Berz, *Taylor models and other validated functional inclusion methods*, IJPAM 4
  (2003) [31] — the form the code already uses.
- Bühler, Barth, *A new intersection algorithm for parametric surfaces based on linear
  interval estimations* (2001), doi:10.1007/978-1-4757-6484-0_15 [4] — thick planes instead
  of boxes for parametric objects.
- Peters, Wu, *Optimized refinable surface enclosures*; Peters, *Efficient one-sided
  linearization of spline geometry (slefes)* — piecewise linear envelopes with provable
  max-norm sharpness, refinable.
- The spline/polygon distance bound of *Tight linear envelopes for splines*, Numer. Math. —
  piecewise linear envelopes from the control polygon.

Candidate tightenings: a centered/mean-value form of the projective step; slefe-style linear
envelopes for the support's spans; carrying the sweep's own closed forms (constant and
Rodrigues frames, docs/procedural-sweep.md §3.2) into the enclosure instead of its NURBS form.
The last has a caution: wr698 found the feet's Taylor models 2.7× slower for a support that is
itself a blend by its definition — a sweep's closed forms are simpler, but the precedent says
measure first.

## What a fix must keep

The target stays `zone/10` (never a loosened tolerance); the goldens are digests, so any
change to a certified net moves every golden case with a free-support blend (upper bound: the
25 blend units, `blend.feature_executive` alone has 150 cases); the exact classes and the
analytic supports must not change at all.
