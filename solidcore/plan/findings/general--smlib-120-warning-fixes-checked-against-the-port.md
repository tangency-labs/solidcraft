# General: smlib#120's warning fixes checked against the port, none changes it (2026-11-02)

Issue: tangency-labs/solidcore#2362. Fork: tangency-labs/smlib#120 (merged at `e8cf987`); the
warning fixes are its commit `9846aa0` (+33/−61 in 11 files). PR: tangency-labs/solidcore#2391.
Checked against `main` at `4e30eb2fd`.

The fork now compiles SMLib clean under the USD build's flags (`-Wall -Wshadow=local
-Wunused-parameter -Werror`). smlib#120 counts 19 warnings; `9846aa0` fixes them at the 18 sites
below (every hunk of the commit). Each was looked up in the Rust port and given one verdict:

- **(a) not applicable**: the port cannot have the defect, or has the fixed behaviour already;
- **(b) observable difference**: the Rust differs from the fixed C++ (none found);
- **(c) not ported**: the C++ the fix touches has no Rust counterpart on `main`.

Result: no site needs a code change. No refusal message differs: the Rust builds its messages
with `format!`, which neither reads data as a format nor bounds what it inserts, and the text it
inserts is the same short text the C++ inserts.

## The 18 sites

Paths are under `crates/solidcore/src/`; line numbers at `4e30eb2fd`.

| # | C++ (`source/SMLib/src/`) | Warning, fix | Rust | Verdict |
|---|---|---|---|---|
| 1 | `SmBallPatch.cpp`: `SmBallPatch::operator==` | `-Wnonnull-compare`: `&crOther` cast with `SM_CAST_NONNULL_PTR` | none: the class is not ported (`blend/blend_corner/ball.rs:38`); only `SmBallPatch::Create`'s refusals are, as `check_ball_patch` (`blend/blend_corner/ball.rs:963`) | (c); and (a): a Rust reference is never null |
| 2 | `SmBlendCorner.cpp`: `SmBlendCorner::SmBlendCorner` | `-Wreorder`: `m_pBallSurface` initialised in declaration order | `BlendCorner` (`blend/blend_corner.rs:98`) keeps only `m_eEvent`/`m_eTurn`; the rest of the corner's members live on `FilletCorner` | (a): Rust has no initialiser list, and every member here is a constant (`NULL`, `FALSE`, `1`), so no order could be seen |
| 3 | `SmBlendCorner.cpp`: `SmBlendCorner::ExactMitreCurve` | `-Wshadow=local`: inner `sW` renamed `sUV` | `exact_mitre_curve`, the seven-point check (`blend/blend_corner.rs:641`-`650`, the point is `w_pt`) | (a): the rename changes no meaning in the C++, and the Rust reads the point it means |
| 4 | `SmBlendCornerBall.cpp`: `SmBlendCorner::BallCentre` | `-Wshadow=local`: `sOn` renamed `sVertex` | `ball_centre` (`blend/blend_corner/ball.rs:542`-`547`, `on` is the vertex's point) | (a): as 3 |
| 5 | `SmBlendSetback.cpp`: `sm_SbGraphG1` | `-Wshadow=local`: `sO` renamed `sOp` | none on `main` (the G1 graph fill is smlib#104, #2310; its open port #2369 has it in `blend/blend_setback/graph.rs`, where the term is a struct field, `af.o`) | (c); (a) for #2369 as written |
| 6 | `SmCrvInVolume.cpp`: `SmCrvInVolume::Dump` | `-Wmismatched-new-delete`: `new TCHAR[32]` freed with `delete []` (real defect, undefined behaviour) | none: `Dump` is out of scope for `curve.crv_in_volume` (`curve/crv_in_volume.rs:113`-`116`) and no unit ports it or `SmGetContinuityTypeString` | (c); and (a): owned Rust strings free themselves, there is no `delete` to mismatch |
| 7 | `SmFilletCorner.cpp`: `SmFilletCorner::CreateCornerPatch` (the degenerate vertex of a 3-edge corner) | `-Wswitch`: `SM_FV_JOIN_SECTION` listed with the kinds that change nothing | `FilletCorner::create_corner_patch_default_with`, the `match` (`blend/fillet_corner/p3.rs:1548`-`1561`) | (a): its `_ => {}` arm already does nothing for every other kind; `FilletVertexType` (`blend/fillet_geom/p1.rs:424`) has no `JoinSection` yet, and adding one lands in that arm |
| 8 | `SmFilletCorner.cpp`: `SmFilletNx1ClosedCorner::MakeCornerTopology` | `-Wunused-variable`: `iDebugLevel` referenced with `SM_REF1` | `FilletCorner::make_nx1_closed_corner_topology` (`blend/fillet_corner/topology/closed_nx1.rs:49`) | (a): the debug level is not ported; there is no variable |
| 9 | `SmFilletExecutive.cpp`: `SmFilletExecutive::CreateFilletBrep` (the ball-check refusal) | `-Wformat-security`: `": no smaller ball fits there either"` passed as `smos_sprintf`'s format, now through `"%s"` (real defect, harmless for this constant text) | `blend/fillet_executive/p3.rs:3472` (`.to_string()`) | (a): no format string; the text is the same |
| 10 | same, the `SM_BS_NOT_ENCLOSED` message | `-Wformat-truncation`: `cWorks` inserted as `%.200s` | `blend/fillet_executive/p3.rs:3491` | (a): `cWorks` is at most `": a radius below "` + one `%.7g` (14 characters at most) + `" fits"`, 36 characters, so `%.200s` never cuts it and the C++ text is unchanged; the Rust `works` is the same text (`c_general(x, 0, 7)` for `%.7g`, `p3.rs:3468`-`3473`) inserted whole |
| 11 | same, the fold message (`... (its touch circles move back)`) | as 10 | `blend/fillet_executive/p3.rs:3496` | (a): as 10 |
| 12 | `SmFilletExecutive.cpp`: `SmFilletExecutive::CreateFilletBrep` (the overflow refusal's open fit) | `-Wshadow=local`: loop `i` renamed `iOver` | `blend/fillet_executive/p3.rs:2936`-`2947` (`for &edge in &overflowing`, solvers by `index`) | (a): as 3 |
| 13 | same, the circle-top fit after `FacesOutlastTheirBlends` | `-Wshadow=local`: loop `i` renamed `iSolver` | `blend/fillet_executive/p3.rs:2984`-`2990` (`for solver in solvers`), the fit itself `circle_top_fit` (`blend/fillet_executive/session/fit_size.rs:738`) | (a): as 3 |
| 14 | `SmFilletKeepFeatures.cpp`: `kf_InMaterial` | `-Wunused-function`: dead, removed | none: `blend/fillet_executive/session/keep_features.rs` never ported it (its table, lines 25-33); `blend/fillet_overflow.rs`'s `in_material` is `sm_InMaterial`, another function | (c): nothing to remove |
| 15 | `SmFilletSection.cpp`: `sm_HermitePoints` (four arguments) | `-Wunused-function`: dead, removed; its comment merged into the five-argument overload's | `form::hermite_points` (`blend/fillet_section/form.rs:147`); the kept overload is `form::hermite_points_sided` (`form.rs:169`) | (a): no behaviour; `hermite_points` is likewise unused by the kernel (only its own test, `form.rs:282`, calls it). Left as it is here (see Follow-up) |
| 16 | `SmFilletSection.cpp`: `SmProceduralBlend::EvaluateDefinition` (the `row` lambda) | `-Wshadow=local`: `sR` renamed `sRow` | `blend/fillet_section/definition.rs:497`-`506` (the closure's own `sr`) | (a): as 3 |
| 17 | `SmPrimitiveCreation.cpp`: `sm_DraftSeamPcurve` (the `fnAt` lambda) | `-Wshadow=local`: out-param `rW` renamed `rWt` | `seam_pcurve`'s `point_at` (`primitive/primitive_creation/p2/taper_walls.rs:532`-`541`) | (a): the closure returns a tuple and binds no name an outer one has |
| 18 | `SmProceduralBlend.cpp`: `SmProceduralBlend::operator==` | `-Wnonnull-compare`: as 1 | `ProceduralBlend::same_definition` (`surface/procedural_blend.rs:428`), which takes `&Self` | (a): a Rust reference is never null, and taking `&Self` fixes the type before the call |

## The two real defects

- **6** (`delete` for `new[]`) is in a debug dump the port does not have, and Rust frees by
  ownership: nothing to port.
- **9** (text as a format) is a constant without `%` in the C++, so it printed correctly before
  the fix too; the Rust never formats it. The same message, byte for byte.

## Follow-up

`form::hermite_points` (15) has no caller in the kernel now that the fork deleted its C++
original. Removing it, and pointing its test at `hermite_points_sided` with `d_in = d`, would
keep `blend/fillet_section/form.rs` in step with the fork's file. That is a tidy-up with no
effect on results and is not done here.
