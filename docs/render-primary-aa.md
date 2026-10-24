# Bounded primary spatial AA proposal (#620 / existing render-quality work)

Source port on the personal rendering branch's CPU/GPU2 snapshot. Written tests
for this recovered unit have not run on this integrated revision; compilation,
layout, PNG, performance and full project validation remain the caller's next
gate. See [advanced CPU integration](render-cpu-advanced-integration.md) for the
exact source baseline and recovery provenance.

`raytrace.primary_samples` is a transient request option: omitted/1 retains the
legacy fixed-centre route,4 uses a2x2 pixel-box grid,16 uses4x4. Other counts
refuse. Default1 is omitted from serialized options to preserve the default
request representation. Nothing is added to document/RenderSetup persistence,
undo, GPU MSAA, material libraries or optical boundary definitions. The integrated
CPU image UI already exposes 1/4/16 primary samples; engine/MCP and the CLI's
runtime raytrace options use the same request contract. This recovered follow-up
changes the bounded reuse implementation, without introducing saved AA state.

The single-primary branch keeps its former generator, reuse proof, R addition
order/divisor, primary-background shortcut, seeds, work debits and encoding.
New modes complete ALL S*R logical attempts, including primary misses and
nonselected surfaces. The current fixed grid certifies repeatability only within
one (pixel, spatial ordinal), so a completed nonselected PRIMARY hit may reuse
its full direct/environment radiance across R. Each reused R still debits one
cancelable scalar continuation, adds coverage once, performs the original
ordered radiance addition and finite-bound check, and keeps the original divisor.
Selected hits (including null reflections), secondary paths and primary misses
retain the uncached route. A completed-state marker uses the existing bounded
light cursor; no task fields, allocations or pools are added. The marker is
cleared before the next spatial sample or pixel; it never crosses jobs. Future
per-R jitter or other varying inputs must revoke the generator proof. The old
blanket assertion forbidding every AA birth proof is replaced by fixed-ray
invariance, distinct-spatial-ray, hit-classification and actual-consumer reference
regressions; observable coverage, progress and refusal assertions are preserved.
R remains the existing reflection attempt count (or1 with no active selections), bounded1..64. Null reflected
continuations contribute zero and still count in R; they never erase primary
coverage. Grid centres are strictly inside the pixel, fixed row-major, without
jitter/temporal sampling or a progressive-prefix claim. New reflection keys use
checked pixel*S+s plus a fixed AA domain xor; the existing sampler receives the
same reflection ordinal, bounce and R as before. No PDF or BSDF is changed.

For geometry hit indicator H and scene-linear path radiance L, C=sum(H)/(S*R)
and P=sum(L)/(S*R). New modes compute conditional geometry radiance G=P/C,
apply exposure/Reinhard once to G, then composite C*T(G)+(1-C)*decode(background)
in display-linear sRGB before one sRGB encoding. All-miss pixels retain the exact
background bytes; full coverage omits background operations. Background remains
display-referred, not an emitter and not exposure-controlled. Do not averageRGB8,
individually tone-mapped paths or tone the premultiplied P. Output alpha stays255;
coverage is internal quadrature, not transparency/refraction support.

One ImageTask/PendingPixel/BVH/host pool and one cumulative1e9 WorkBudget remain.
AA retains fixed scalar/Vec3 accumulators; reuse adds no fields. Per-R-add
radiance limits remain unchanged;
spatial normalized sums have a finite1e12 bound. Coverage is checkedu16<=1024.
Separate spatial finalization and pixel emission states each debit work and yield;
cancellation is checked again just before pixel bytes are emitted. Running
progress still counts completed OUTPUT pixels. No partial PNG or samples reduced
on exhaustion. Existing384MiB job/512MiB host limits and PNG encoder are retained.
A compile-time type-length check and written layout regression require actual
ImageTask<=4096 bytes; because no build has run, actual layout is NOT qualified.

Cost can grow with S*R, traversal, lights, HDR and bounces. Retaining exact
intra-spatial primary reuse does not establish any work-budget success. Limits
can refuse jobs that previously completed. No work/ray equality, speedup, smooth
metal, universal antialiasing or final-mode claim follows. Thin/repeated geometry
can still alias under fixed quadrature, and reflection noise is a separate issue.

Nine written regressions cover strict options/default serialization; grid and
analytic discrete edge coverage; conditional tone/display-linear compositing;
actual hit-first/miss-first half-plane traversal acrossR1/4/16/64 with nulls;
all-miss bytes and quantum parity; full coverage; cancellation/work exhaustion
in later spatial samples/finalization/emission; and S1 isolation/bytes/work with
same caps/layout. The blanket birth-proof policy assertion changes as described
above; existing observable assertions remain. New actual-consumer tests compare
every ordered R accumulator prefix, logical coverage and PNG bytes with the same
ImageTask route whose proof is revoked before its first shading completion.
Execution, type/layout checks and all acceptance gates are pending.
Default-vs-historical exact-build byte/work comparison, engine/host preflight
validation, arbitrary projection scenes, allocation/layout and ownership gates,
Clippy/wasm and required cargo xtask ci are required before publication. Designer
must inspect actual equal-camera edge comparisons; source math is not visual
acceptance. No external code/assets/dependencies were added.
