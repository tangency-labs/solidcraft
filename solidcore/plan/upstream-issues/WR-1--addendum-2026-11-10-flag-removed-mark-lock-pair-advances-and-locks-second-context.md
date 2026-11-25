Updates: WR-1

#### WR-1 (update, 2026-11-10). `mark_lock_pair_advances_and_locks_second_context` is removed

solidcore's two-context mark lock (`MarkLock::new_pair`, upstream `SmNewMarkAndLock(SmContext*, SmContext*)`) advances and locks the second context's own free mark, so releasing every lock leaves no mark locked. This is now the only path, and the flag is gone. Upstream advances the first context twice and locks the first mark type in the second context, while its destructor unlocks the second mark type there. SolidCraft already ran with the flag on. Removed: the field, `MarkLock::new_pair_with_fixes`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** `geom.context` changes in 763 of 2013 cases, every one of which uses a pair lock. Every case's final snapshot is taken after all locks are dropped. There, upstream's behaviour leaves a stray lock in 164 cases; solidcore's leaves none in any of the 2013.
- **Tests:** `geom::context` unit tests `wr1_two_contexts_each_advance_and_lock_their_own_mark` and `wr1_same_context_and_out_of_bounds`.
