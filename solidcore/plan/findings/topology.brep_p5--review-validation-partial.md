# topology.brep_p5 — review validation (partial)

PR #194 restores all twelve owned editing bodies and the internal pending-topology
attribute cleanup. Source `a9f20328` includes functional official main `43e9e47a`; no open
dependency PR is included. Final main `85a60e00` differs only in generated
`PROGRESS.md`; build and fixture files remain identical to the tested source.
The earlier pre-review evidence above is historical.
All owned continuations are present with per-call missing-dependency barriers,
including combine tails, shell/vertex-use cleanup, curve joins/UV cleanup,
manifold/face cleanup, the second stitch pass and orientation override, squeeze
output lifecycle, transformation/reflection and tolerance work.

Review fixes include the declaration's infinite-region default
`SM_VS_INFINITEREGION_SWAP` (2), same-input glue before notification (including two
null inputs), and aliased combination on the native path. The second review round
corrected the isolated-alias case: upstream deletes the aliased survivor and returns
success without touching it again, so the whole combine is defined and compared.
Only the target-stage op, whose shim reads the survivor afterwards, refuses it (a
native incidence query). The caller-side hazard is recorded as UB-191 in
`plan/upstream-issues.md` (source finding; no native reproduction).

The boolean intersector's insertion calls the shared part 5 control flow with
actual editing/tolerance getters, then defers at `FindVertexSector` (face part 4)
and `MakeEdgeInFaceWithVU` (B-rep part 9). Landed shell/region queries, list operations, wire traversal
and child transfer are wired. Shell-record destruction still needs stable arena
erasure and actual lifetime/key routing from B-rep part 4 (`SmBrep::~SmBrep`);
the shared arena is append-only. This is not a missing `SmShell` destructor body.
Opaque glue callers share the identity
preflight; nonalias edits still need the mutable owning arena/kernel and part 2's
notification routing. When that adapter lands, it must call the existing complete
`glue_with_notifications` body, not just replace the preflight barrier with Notify.

Healer adapters capture actual context notifications instead of inventing removed
and added lists. Supporting co-port in topology.topology:
`SmTopologyChangeCallback::{Execute, AddUnique, Remove}`. The tracker and callback
share notification arrays (no topology graph pointers), preserving active reset
and foreign-callback replacement lifetimes. Geometry owner and edgeuse-to-edge
metadata must come from the actual object store; missing metadata is an explicit
capture failure, and chains are bounded to 64 links. Nonempty arena-only healer
captures still require part 2's actual key-to-id routing; no arena-index encoding
is guessed. All 27 native callback scripts pass, including list order, event
payload identities, destruction cleanup, curve-owner traversal, direct face
notifications and tracker lifetimes. A preexisting system callback is preserved;
the new native tracker leaves its own arrays empty. Three native cases and a
healer regression cover this behavior without fabricating changes.

Following [issue #210](https://github.com/tangency-labs/solidcore/issues/210),
**meaningful successful topology-mutation paths certified: 0/12 operations**.
All twelve source-level control flows are present; that source review is distinct
from runtime certification behind missing calls. Part 1's canonical editing fields
remain an integration handoff after its independent PR merges. Part 2's Notify
remains the critical dependency; its open PR is not bundled here.

Coverage on source `a9f20328`, Linux runner 2 (Ubuntu 26.04 x86_64, Rust/rustdoc
1.98.0, GCC 15.2.0; pinned `c9c5979` reference, C++17/O2 with
`SM_USE_OLDTOL`, FP contraction and fast math disabled):

- **791 cases:** 279 fixtures + 512 generated; **737 pass, 54 deferred, 0 fail**,
  max relative error 0, fixed relative tolerance 1e-9, no absolute floors.
- **409** passes compare only the coincident-vertex numerical target against a
  real native merge's survivor. They do not certify the topology edit or directly
  compare the temporary-edge minimum-length threshold.
- **301** compare whole-operation refusals/output lifecycle, empty selections,
  identity transforms and aliased-glue no-ops (with unchanged semantic graphs).
- **27** compare supporting notification algorithms; they do not mutate topology.
- Deferred counts: B-rep part 1 **10**, part 2 **40**, part 4 **2**, part 9 **2**.
- Sensitivity: **737/737** detected, 0 unperturbed, 54 deferred.
- Extended 4096 generated: **4375 total, 4195 pass, 180 deferred, 0 fail**,
  max relative error 0.

Formatting, strict plain/native Clippy, all **1331** kernel tests, WASM and all
**3** native FFI guard tests pass. Eleven affected differential units pass their gates
(1 green, 10 partial, 0 failed). The full green/partial sweep also passes:
**204 units: 113 green, 91 partial, 0 failed**. No additional unit needs promotion. Commands
use `nice -n 10`, one Cargo job and sccache. The matching 1.98 rustdoc is selected
explicitly. A worktree-owned temporary 4 GiB swap file supported compilation on the
8 GiB runner and was removed after validation. No permanent runner configuration
was changed.

Uncertified: successful topology mutations, nonidentity transformations,
continuations behind dependency barriers, native geometry shared across separate
owning records, longer/mixed geometry owner chains, and pending-reader integration
(part 6). Separate Rust geometry slots own separate values; equal owner metadata
is not proof of native pointer identity. Pending cleanup has focused Rust tests
for attribute detachment versus release, with no standalone native reader oracle.
No temporary records, attributes, keys or successful mutations substitute for
caller state. This unit is **partial, never green** with these deferrals.
