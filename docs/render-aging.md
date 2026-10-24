# Saved surface aging

## Stable saved targets

Saved `job.options.aging[].target` uses `definition`, `feature`, `component` and
`occurrence_path`, the same identity contract as saved surfaces and optical bindings.
The read-only `render.scene` reply exposes `stable_target` beside each captured body.
The editor saves that identity, not `ordinal` or an unqualified visible name.
Check and Start resolve it against the current scene; a hidden, removed or replaced
occurrence requires explicit rebinding. Another body's insertion/reordering does not
retarget a layer. The runtime-only GPU job still receives its captured index/name pair.
The procedural field origin remains world-anchored; this does not add object-local
weathering motion or change physical geometry, material cards or mass.

Untargeted experimental layers retain their previous fields and meaning. An old
`body_index`/`body_name` target cannot be inferred safely across captures and is
explicitly refused with a rebinding message. Keep the original file and rebind in a
compatible development build; never strip the target or silently make it global.
`format/pending/render-stable-aging-targets.rs` records the new saved concept without
editing published numbers or migrations.
