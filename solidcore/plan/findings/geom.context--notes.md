# geom.context

- **Two-context `SmNewMarkAndLock` constructor bug (ported as is).** In
  `SmNewMarkAndLock(SmContext* p1, SmContext* p2)` with `p1 != p2`, upstream picks
  `m_eMarkType2 = GetUnlockedMark(*p2)` but then calls `m_pContext1->NewMark(m_eMarkType1)`
  (advancing context 1's mark a second time, not context 2's) and
  `LockMark(m_pContext2, m_eMarkType1)` (locking mark type 1 in context 2, not type 2).
  The destructor then unlocks `m_eMarkType2` in context 2, so when the types differ a
  stray lock of type 1 stays on context 2. `geom::context::MarkLock::new_pair` reproduces
  this exactly (difftest case `boolean_pair_two_contexts`). `SetContext(p1, p2)` is
  correct (locks type 2 in context 2) and does not advance marks.
- **Release build ignores locks.** `NewMark`'s lock checks and `LockMark` /
  `UnlockMark`'s state checks are `SM_ASSERT*` (messages only), so locked marks can still
  be advanced and double locks are silent; the port drops the assertions.
- **`SmNewMarkAndLock` binds contexts by index.** Rust `MarkLock` stores indices into a
  caller-owned `&mut [Context]` instead of pointers and its destructor is the explicit
  `release`; `with_mark_lock(&mut Kernel, ..)` is the scoped form for the kernel's own
  context (`Kernel::context`). Topology porters: use `k.context` for
  `GetContext()->GetCurrentMark(..)` and `with_mark_lock` for `SmNewMarkAndLock`.
- **Build flags.** The reference build has neither `SM_USE_GLOBAL_CACHE` nor
  `SM_USE_NEWTOL`, so the constructor is the two-double one and
  `SetThisModelSizeEstimate` is the inline header version
  (`m_sThisZoneTol3d = SizeEstimateToZoneTol3d(estimate)`, which maps `SM_USE_DEFAULT` to
  `SM_MODEL_SIZE_ESTIMATE`, not to the system statics).
- **`SmApiGetOrCreateContext` global.** The SM_API keeps one process-global context;
  the port takes the `Option<Context>` slot from the caller.
