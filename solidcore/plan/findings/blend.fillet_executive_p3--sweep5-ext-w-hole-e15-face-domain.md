# blend.fillet_executive_p3: sweep5 ext_w_hole e15 face tolerance and UV domain (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Input: `sweep5-ext_w_hole-e15-r1.0` (`Fillet/ext_w_hole.smb`, edge 15, circular G1, radius
1.0, thumbweight 1).

Values: status and topology counts agree. 43 `result_geometry` values of one face differ:
its tolerance (`[442]` C++ 5.297559784473877e-5, Rust 4e-5), its UV domain minimum
(`[443]` 0 vs 2.5, `[444]` 0.5868844001777145 vs 7.5) and so its surface samples (`[447]`
7.505019722691157 vs 7.667949705662158, ...). The rail trims agree ((2.5, 7.5) both sides).
So do the per-stage UV-trim counts and lengths from CreateFilletBrep to the end of merge
(final 10 faces, 48 edge uses, 13 without UV trim, total length 154.460664419).

Suspected cause: the face's final shrink / UV domain update (`SmFace::ShrinkGeometry` or a
tolerance raise from its UV trims) takes a different branch. C++ shrinks the domain to the
face's trims and raises the face tolerance. Rust keeps the rail-trimmed domain and the
default tolerance. Not localised yet.
