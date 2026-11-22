#### WR-1. Two-context `SmNewMarkAndLock` advances and locks the wrong context
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `source/SMLib/inc/SmContext.h:528`-`529` (binary constructor).
- **What happens:** With `p1 != p2` it calls `m_pContext1->NewMark(m_eMarkType1)` (context 1 advanced twice) and `LockMark(m_pContext2, m_eMarkType1)` (wrong mark type). The destructor unlocks `m_eMarkType2`, so a stray lock stays on context 2.
- **Trigger:** `SmNewMarkAndLock(c1, c2)` where the free mark types of `c1` and `c2` differ.
- **How found:** adversarial / edge-case fixture `boolean_pair_two_contexts`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (`MarkLock::new_pair`).
