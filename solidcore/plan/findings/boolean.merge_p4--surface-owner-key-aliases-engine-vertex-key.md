# boolean.merge_p4: surface-attribute copy hazards: owner names alias engine vertex keys, and persisted surface attributes are dropped (2026-11-02)

Units `boolean.merge_p2` (`MakeFaceuse`'s surface-attribute copy) and `boolean.merge_p4` (`kernel_adapter`'s merge stores). This is a latent hazard found by code reading during the feature-executive attribute-carry work. No case is known to reach it, and no code changes with this note.

## The keys

- `MakeFaceuse` (`boolean::merge::p2::make_faceuse_attempt_with`) copies the other face's surface attributes with `copy_surface_attributes`, the port of the surface copy in `CopyAndAddAnalytics`. It reads the source surface's lists under the surface's **owner name**: the owning face's `ObjectRef` in that B-rep's naming. For a B-rep with notify naming, that is `topology::loopuse::notify_ref(Face(j))`, key `1 + (4 << 32) + j`.
- The engine store of `boolean::merge::p4::kernel_adapter`, with namespaced keys, names primary objects `notification_key(namespace, false, entity)`, key `(namespace << 37) | (0 << 36) | (kind << 32) | index`. Vertices are kind 4, so a namespace-0 primary vertex `Vertex(j + 1)` has key `(4 << 32) + j + 1`.
- Those are the same bits. If an operand with notify naming has its face surfaces owned under `notify_ref`, then `copy_surface_attributes` in an engine-keyed merge reads primary vertex `j + 1`'s list as face `j`'s surface attributes and copies it to the new face's surface.

## Why it is only latent

- Upstream's surfaces are objects of their own, so this aliasing has no upstream counterpart.
- The merges' genuine surface lists (`SM_AI_SURFACEMAP`, `SmMerge.cpp:415-434`) are keyed by the owner names. That is what the surface copy is for, so the copy stays on in every `kernel_adapter` site (the `surface_attributes` switch of `TopologyCopy`).
- No case in the gate is known to give a primary vertex an attribute list in a namespace-0 engine store while copying a face whose owner uses notify naming.
- The feature-executive carry fix (#2377) switches the copy off only where the store's lists can only be the destination's (`topology::brep::p7::copy_faces_between_stores`, `boolean::merge::p3::copy_component`). Those collisions were seen in batches 54k95 and 54k96.

## What a fix would need

Surface identities that no topology key can share. For example, a surface namespace for owner names in the merge stores, or a type-code check in `copy_surface_attributes`. That is a change to the merges' surface identities. It should come with a case where a vertex attribute list and a notify-named operand meet in an engine merge.

## Related open item: persisted surface attributes and the switch-off

The carry fix (#2377) switches `MakeFaceuse`'s surface-attribute copy off in `topology::brep::p7::copy_faces_between_stores` (the feature-executive patch copies) and in `boolean::merge::p3::copy_component`. Upstream gives a surface object attributes of its own on these paths in one way only: attributes persisted on a surface record of a model file (`SmBrepData.cpp:5739`, versions > 30). The other places are `SM_AI_SURFACEMAP` (`SmMerge.cpp:415-434`, only with `pSurfacePairsToSkipSSI != NULL`, which the feature executive doesn't pass), and fillet and primitive operations off these paths.

The switch-off would drop persisted surface attributes on the feature-executive patch path and in `copy_component`. This is untested: it needs an `.smb` fixture with a surface attribute table. None of the `blend.feature_executive` fixtures has an attribute table. The first step is to trace where the port's reader (`topology::brep_data::read_surface_from_db`, which keys them under the surface's own read key) leaves such attributes in the B-rep store.
