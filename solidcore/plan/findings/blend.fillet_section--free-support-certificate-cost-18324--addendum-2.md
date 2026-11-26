# Addendum 2: the golden movement was the machine, not the memo

Addendum to `blend.fillet_section--free-support-certificate-cost-18324--addendum-1.md`
(same claim: tangency-labs/solidcraft#18324, PR #22347). It corrects that record's golden
story; nothing else in it changes.

## What addendum 1 got wrong

Its closing section attributes a movement of 69 golden cases across five blend units to
the memo sharing, through a mechanism it states as "a kernel's copies can re-derive a
slightly different valid bound than their parent". That mechanism was never observed: it
was the explanation offered for movements that turned out to be something else.

## What the measurements actually show

The five units that moved when the branch first ran locally — 1 case in
`blend.feature_executive`, 2 in `blend.fillet_corner_p2`, 63 in `blend.fillet_executive_p3`,
2 in `blend.fillet_solver_p3`, 1 in `blend.hcr` — move identically on unmodified main on
the same machine: the same cases, the same values, while main's own CI, whose runs blessed
the goldens, passes every one of them. The movement is the machine against the golden's
blessing platform, not the memo. Ablating the sharing (copies and pools unshared again)
leaves the moved values where they are, which rules the memo out directly.

The re-blesses that addendum 1 records are reverted: the branch keeps main's goldens, and
the memo sharing's golden footprint is zero as far as any measurement reaches. Whether a
copy ever re-derives a different bound than its parent remains unobserved either way; the
design's rule that any kernel takes any body and gives the same answer says it cannot, and
the bit-for-bit identity of the reported design's volume, area, face and edge counts on
both platforms is the evidence the sharing introduces no difference of its own.

## The dual-platform effect, on the way

The fix's measured effect on the reported design, the branch against the same base with
the memo commits reverted, cache dropped, three runs each: 20.8-21.4 s to 16.2-16.7 s on
an Apple M4 (−22%), 46.5-47.2 s to 36.0-36.2 s on a Ryzen 7 3700X (−23%). The design stops
at Fillet3 on main from tangency-labs/solidcraft#22418; both sides of each comparison ran
with `--allow-errors`, so the comparison is like for like.
