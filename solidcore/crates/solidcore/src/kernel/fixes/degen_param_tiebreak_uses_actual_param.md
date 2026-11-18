`SmSurface::FindDegenParamForDirection` compares the actual singular parameter
mapped into `sSIvl` (`s_ivl.evaluate(this_param)` and
`s_ivl.evaluate(near_best_param)`) with the caller's singular parameter `dS` in
the "nearly as good and closer to the input" tie-break, so the nearest candidate
starting guess is chosen on degenerate boundaries whose singular domain is not
`[0, 1]`; pinned v1.0.1 compares the `[0, 1]` sample fractions `dThisParam` and
`dNearBestParam` directly against `dS` (WR-57, #2209;
`plan/upstream-issues/WR-57--addendum-2026-11-01-degen-param-tiebreak.md`). Off
by default for pinned C++ parity.
