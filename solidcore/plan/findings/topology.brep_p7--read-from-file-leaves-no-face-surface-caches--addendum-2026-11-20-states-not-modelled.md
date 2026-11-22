# topology.brep_p7: the read's face-surface cache states are no longer modelled (2026-11-20)

Updates: plan/findings/topology.brep_p7--read-from-file-leaves-no-face-surface-caches--addendum-2026-10-28-renumber-cache.md

- **The states are gone.** The (no cache, unbuilt, built) states counted there are no longer kept on the surface. `Surface::copy_cache_slot_of` now copies only the kept trimmed cache (its handles renamed, its face binding rekeyed); there is no state left to copy.
- **What the states used to decide.** `Endcap.smb`'s cache made during the read without a tree made `GetDomainWithContinuity` answer the natural domain in `offset.offset_geometry_creation` gen0266. It now answers the surface's own continuity domain.
- **gen0266 now.** `LiftCurve` refuses the segment (status 1001), because the segment lies wholly off the surface's domain. The old lift was 1.8e10 off the surface. The edge keeps its curves.
- **Four other cases changed: gen0152, gen0462, gen0678 and gen0760.** In each, the lift's cache request used to build the face's tree with its UV trim curves on the body. The lift no longer requests a cache, so the edge uses recorded after `ExtendTrimEdges` have no UV trim curve. In gen0462 and gen0760 the edge also keeps its tolerance, which the trim curve creation had raised (0.99 to 2e-5, 1.08 to 1e-5). No status and no 3D geometry changes.
- **Details:** `plan/findings/surface.surface_p1--surface-cache-history-modelled-issue-568-follows-surface--addendum-2026-11-20-history-removed.md`.
