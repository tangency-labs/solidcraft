#### UB-163. Uncached healer props removal leaves targets that resize into NULL props holes
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmHealData.cpp:1332-1333` (`RemoveVertexProps_FromLists` and analogous edge/face helpers), `:1224-1226` (`UpdateTargetLists`); later `Cache_CoinVertices` / `Cache_DegenFaces`.
- **What happens:** removing a props record whose topology pointer has never been cached removes the props but not its target. A later nonempty add list resizes every props array from the target count and initializes only the new-target tail, leaving NULL holes for the unmatched old targets. Native coincidence/degeneracy caching can then dereference invalid state. Even a null add causes that resize. A later `Get*Props_ForTgts` can also index past a props list shortened by an uncached removal.
- **Trigger:** box with all targets, no caches: set done=9, update through 6, remove vertices 0/1 and add vertex 0 twice. Another trigger: remove uncached face 0 and add NULL. With edge/vertex caches, done=12 and update through 9, removing/adding uncached face 0 reaches `Cache_DegenFaces`. A prior `Remove*Props_FromLists` can introduce the same drift.
- **How found:** crash in the C++ reference run during the maintainer's step-33 sequence fuzzing ([PR #164, round 2](https://github.com/tangency-labs/solidcore/pull/164#pullrequestreview-5554396459)); source inspection confirmed the mismatched list sizes and resize boundary.
- **Confirmed:** yes by that reference run: 31 crashing scripts (7 coincidence-cache, 21 harness reads of NULL props, 3 degeneracy-cache). Requires inconsistent caller-controlled healer state; ordinary end-to-end healing was not the trigger.
- **Severity:** crash/UB.
