#### WR-266. A failed `SetProps_Stage3` retains its in-progress marker and skips retry
- **Severity:** Wrong result
- **Area:** Topology and healing
- **How found:** Source audit of #521, with the regression/reference evidence below.
- **Location:** `source/SMLib/src/SmFaceProps.cpp`, `SetProps_Stage3`, `m_ePropStage = SM_PROPSTAGE_3a` and its early `>= SM_PROPSTAGE_3a` guard.
- **What happens:** an error leaves Stage3A set, so a later call can return success without completing the stage.
- **Confirmed:** yes; a pre-fix Rust regression fails on retry after a dangling loopuse. Four actual native comparisons retain error 1040 on both sides but expose marker 5 (C++) versus 4 (Rust); other compared outputs agree. No live C++ dangling-loopuse probe was run.
- **Rust port:** restores Stage2 on error without clearing partial diagnostics, so the next call retries. Four diagnostic differences stay explicit known records and the healer unit remains partial. #521 / PR #700; findings: Failed topology evaluations and incomplete healing stages.
- **Findings:** plan/findings/general--521-topology-evaluation-status.md

**Current validation:** Eight existing stage-marker differences are explicit known
records; the latest targeted run has 1,520 matching and eight known cases, no
deferrals or unexpected failures. See plan/findings/general--521-topology-evaluation-status--addendum-2026-10-25-cache-owner.md.
