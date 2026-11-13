#### WR-167 (addendum 2026-11-05) — Correct pole mask membership by default

Updates: WR-167

The Rust pole lookup now always tests each point's corresponding mask bit; the
`masked_array_uses_bit_shift` compatibility flag and comparison path are retired.
Indices 64 and above have no representable bit. Point equality remains approximate.
The default now recognizes selected UMIN and VMIN points and selected UMAX and VMAX
points independently of UMIN; UMIN alone no longer selects UMAX or VMAX.

The regression exhausts four-bit masks, checks absent and duplicate points and the
63/64/69 boundaries, and exercises all four sides and both UMIN false-positive cases
through the bisector. This is a discrete membership oracle, not a C++ confirmation:
the original record's reference comparison was deferred, and no affected C++ case
has been verified here.
