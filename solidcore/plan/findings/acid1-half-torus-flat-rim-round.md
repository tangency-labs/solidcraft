# Half-torus flat-rim round

Reported in tangency-labs/solidcraft#19182. The Acid-1 stage 4 reproduction is
`crates/solidcore-ops/tests/blend.rs::both_flat_face_loops_of_a_notched_half_torus_round`.
It builds the reported major-30, minor-8 torus, removes z < 0, joins the
radius-0.95 boss, cuts the radius-0.7 notch and rounds the flat face's two loops
at radius 0.8. Bodies pass through records between operations, as in SolidCraft.

## Baseline

The installed `solidcraft-cli 0.44.1` reproduces the original command script,
including default tangent propagation: `timeout 120 solidcraft-cli run
/tmp/solidcraft-19182/repro.json --results --timings --profile` exits 124.
The script containing just the first four commands renders successfully with
`solidcraft-cli snapshot`; the whole model and two close views of its notch
are in `/tmp/solidcraft-19182/`. Thus those booleans build before the fillet
stalls. This replay uses the exact reported release.

Additional probes of the same installed release isolate the outer loop:
rounding only the inner rim on the notched input completes (24.0 seconds in the
blend call); rounding both rims of the plain half torus completes (30.2 seconds
in the blend call). Rounding the notched input's outer selection alone still
exits 124 at 120 seconds. These times locate the reported failure; they are not
test assertions.

The three booleans finish, and the picks resolve to native edges [2, 10, 4].
An initial replay of those three picks, before explicitly expanding SolidCraft's
default two-degree tangent chains, still had not returned after more than 200 seconds in the unoptimized
test profile; its buildq job was cancelled after 375 seconds including the
130-second compilation. No geometric refusal or result was returned. The test
now explicitly expands those chains to reproduce the command's default.

A CPU sample of an available 0.45.0-rc.2 diagnostic CLI (not this worktree's
validation binary) locates 2964 of 2965 samples in
`GnSectionFsg::build_given_ball`, 2956 in `GnSectionFsg::certify`, and 2128 in
`gn_certify_span_reusing`. All are under `fillet_solver/p4.rs`'s
`do_point_intersection` / `c4_trace`: the expensive work precedes the corner
intersection. The sample used the system's gperftools signal-driven CPU profiler,
not a debugger; its raw profile, stacks and resolved summary are in
`/tmp/solidcraft-19182/newer-cpu.*`.

The exact classifier excludes every torus support before checking for a common
axis (`blend/fillet_exact.rs`, `classify_blend_spine`). For the reported flat
rims, however, the offset torus's meridian circle and the offset plane's horizontal
line intersect in two exact points. Revolving each point gives the existing
`Circle` spine class. Its exact builder runs before `c4_trace` and stores the
ball's section as a rational revolution. The construction includes the
torus's axis, meridian circle and exact contact foot; it leaves noncoaxial tori
and collapsed centre circles with their existing constructions. It adds no
tolerance and does not alter certification.

The corrected source replay passes with the full propagated selection [2, 10, 4, 5].
It records zero generic span certifications, rounds all four selected edges, and
passes the closed-solid, complete topology, point-classification and independently
derived volume checks. The unoptimized test finishes in 25.86 seconds (14.69 seconds
in the blend call); times are evidence only, never assertions. Its native before
and after bodies are saved in `/tmp/solidcraft-19182/` for the matching renders.
The final regression retains a zero-certificate assertion under the `profile`
feature; the temporary timer and file writes have been removed.

The matching native renders use `solidcore-diag --binary --view 180,35
--size 1280x720`. They are `flat-face/native-before.png` and
`flat-face/native-after.png` under `/tmp/solidcraft-19182/`; the latter highlights
new faces 6–9. Both render with zero refused faces. The renderer reads the actual
saved bodies; no geometry is reconstructed for the pictures.

## Validation

The queued source regression command passed (one test, zero failures):

    buildq run solidcraft . -- cargo test --manifest-path solidcore/Cargo.toml -p solidcore-ops --features profile --test ops blend::both_flat_face_loops_of_a_notched_half_torus_round -- --exact --nocapture

The passing run included temporary timing, profiling and body-save diagnostics.
Those diagnostics were then removed; the final test asserts the measured zero
generic span certifications when the `profile` feature is enabled. Both final
format checks completed successfully:

    buildq run solidcraft . -- cargo fmt --manifest-path solidcore/Cargo.toml --all
    buildq run solidcraft . -- cargo fmt --all --check

`git diff --check` also passed. The additional classifier unit tests,
`error::phrase_sites`, `error::blend`, related torus regressions and clippy for
`solidcore` and `solidcore-ops` were cancelled before execution (exit 130) under
the updated standing instruction to stop after the fix compiles and its own
regression passes. Those checks, the cleaned regression rerun and the full suite
remain for CI. No main files were merged or copied; changes remain unstaged.

## Independent volume derivation

For the half torus alone let M = 30, R = 8 and r = 0.8. In its meridian plane the
ball centres lie at height r and radial distances M +/- a, where
a = sqrt((R-r)^2-r^2) = sqrt(51.2). The rails on the original torus lie at
height h = Rr/(R-r) = 8/9. Summing both corners, the removed meridian area is

    A = (R^2-r^2) asin(r/(R-r)) - r a - pi r^2/2
      = 0.32492299709527694.

Reflection about radial distance M makes the two removed areas' first moment
M A. Pappus therefore gives the rounded half torus volume

    V_ring = pi^2 M R^2 - 2 pi M A = 18888.393908052152.

The boss is wholly under the flat face, far from either rounded rim, and adds
pi (0.95)^2 (1.9) = 5.387046002743097.

For a mitred meeting of the rounds, each horizontal section of the notch removes
the intersection of the ring's outer disc and the notch's disc, with centres
38 apart. The inner rim is too far away to intersect that notch. The outer
disc radius S(z) and notch disc radius q(z), for 0 <= z <= 3, are

    S(z) = M+a+sqrt(r^2-(z-r)^2),  z <= h;
           M+sqrt(R^2-z^2),       z >= h.
    q(z) = 0.7+r-sqrt(r^2-(z-r)^2), z <= r;
           0.7,                    z >= r.

The notch stops at z = 3, its cutter's top. With d = 38,
x = (d^2+S^2-q^2)/(2d), y = d-x and k = sqrt(S^2-x^2), the removed section is

    L(S,q) = S^2 acos(x/S) + q^2 acos(y/q) - d k.

Integrate L(S(z),q(z)) from 0 to 3, splitting at r and h. Removing the endpoint's
square root with z = r(1-cos t) on [0,r], and using z = r(1+sin t) on [r,h],
composite Simpson integration with 2048, 8192 and 32768 panels per interval
gives respectively 1.587564298424581, 1.587564298424715 and 1.5875642984246443.
Thus the independently derived mitred result is

    V_ring + V_boss - V_notch = 18892.19338975647.

No value is taken from a kernel result or a commercial CAD product. These numbers
do not change a golden or a kernel-case expectation.

An independent 45-digit Decimal calculation of the same integrals gives
1.58756429842459800 for the notch and 18892.19338975647310 for the complete
result at 2048 panels per interval.
