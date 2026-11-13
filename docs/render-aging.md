# Saved surface aging

## Stable saved targets

Saved `job.options.aging[].target` uses `definition`, `feature`, `component` and
`occurrence_path`, the same identity contract as saved surfaces and optical bindings.
The read-only `render.scene` reply exposes `stable_target` beside each captured body.
The editor saves that identity, not `ordinal` or an unqualified visible name.
Check and Start resolve it against the current scene; a hidden, removed or replaced
occurrence requires explicit rebinding. Another body's insertion/reordering does not
retarget a layer. The runtime-only CPU or GPU job receives its captured index/name pair.
The procedural field origin remains world-anchored; this does not add object-local
weathering motion or change physical geometry, material cards or mass.

Untargeted experimental layers retain their previous fields and meaning. An old
`body_index`/`body_name` target cannot be inferred safely across captures and is
explicitly refused with a rebinding message. Keep the original file and rebind in a
compatible development build; never strip the target or silently make it global.
`format/pending/render-stable-aging-targets.rs` records the new saved concept without
editing published numbers or migrations.


## CPU evaluation contract

A CPU raytrace job can preserve the same optional `options.aging` stack and stable
saved targets. Empty or absent stacks retain the preceding saved JSON and CPU PNG
arithmetic. An inactive stack retains its bounded parameters without adding a
material continuation or consuming extra transport work. Check and Start refuse
missing/replaced/hidden targets explicitly. This is optical authorship: engineering
material cards, geometry, visibility, emission and mass are unchanged.

An active stack is evaluated once for each hit before the shared opaque closure.
Direct lighting, opaque path response/PDF/sampling and first-hit denoising guides
use that same result. The worker reuses one fallibly reserved, prepaid material
slot for the guide prepass and then transport. Each layer is a charged resumable
continuation; cancellation or cumulative-work exhaustion cannot advance that
layer or commit an incomplete camera pass. No extra image plane or RNG stream is
introduced. The raw EXR captures the aged transport before denoising/presentation.

Layer order, original pre-aging metallic class, exact body targeting, upward bias
from the original outward geometric normal, RGB/roughness/metallic changes and
clear-coat weight attenuation follow the existing ordered layer contract. Coat
roughness and IOR, shading/geometric normals, emission and topology are preserved.
World millimetres are evaluated in f64 after subtracting the explicit authored
origin; the camera never defines the CPU field. Noise repeats every 4096 cells,
while directional Scratch bands use their separate `floor(0.8*q)` identity.

Certified dielectric boundaries and passive stage geometry are not aged. An
explicit certified-glass target is refused. CPU aging cannot replace independently
authored anisotropic roughness axes; that combination retains its values and is
refused explicitly. The legacy selected rough-reflection HDR path also refuses
layers that would introduce a diffuse lobe it cannot carry; keeping metallic aging
or explicitly selecting opaque path transport avoids silently discarding energy.
Other existing transport/geometry/coat/HDR restrictions remain in force. This
consumer does not add CPU texture-map support or broaden dielectric transport.

`format/pending/cpu-aging.rs` records the additional CPU saved concept. The staging
artifact is source preparation only: its newly added tests, target layouts, native
CPU jobs, UI controls and saved reopen journeys are UNCOMPILED/UNRUN until the
integration branch's serial qualification executes them. GPU Scratch camera-rebase
precision/phase work is tracked separately and is not a CPU-parity validation claim.
