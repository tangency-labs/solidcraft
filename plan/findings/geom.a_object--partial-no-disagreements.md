# geom.a_object (partial; no disagreements)

The difftest has 2040 default cases: 8 fixtures derived from upstream tests, 32 edge-case
fixtures and 2000 generated cases (types, NLI helpers, notify/attribute/cache scripts).
A 50,040-case run (`--seeds 50000`) was also clean. Every case that is not deferred
matches upstream exactly. The deferred cases are on `topology.brep_cache` (a new
`SmBrepCache`), `topology.shell` (`SmRegion::IsVoid` in region propagation) and
`topology.topology` (`SmSAGObject::Notify` ends in `SmTopology::Notify`).

Notes for later agents:

- **Data model.** `geom::a_object::notify` drives the `AttributeStore` handlers of
  `geom.attribute`. Pass it the notified object as an `ObjectRef`; region-ness is
  `type_code == SmRegion_TYPE`. Caches live in `CacheStore<T>` and `AObject` holds
  the slot. A class's `CacheMakeOrValidate` override is chosen by passing a
  `CacheMaker`. The curve and surface makers are `UNPORTED(curve.curve_p5)` and
  `UNPORTED(surface.surface_p1)`.
- **Region propagation (`SM_NO_REG_PROPAGATION`) is quirky upstream. It is ported
  as is but stays behind `UNPORTED(topology.shell)`:**
  - The source loop runs twice. The second pass resets `sAttributes1` and keeps
    `sAttributes2`.
  - `lAttrib2Size` is taken *before* `GetAttributes(sAttributes2)` refills the
    list. On the first pass the other region's attributes are therefore never
    visited. With two or more other regions, upstream indexes the refilled list
    with the previous region's count, which reads stale or out-of-range entries.
    Rust returns `InvalidInput` where the index is out of range.
  - `SET_FROM_OTHER` removes the target attribute while the snapshot still holds
    it. With a duplicate list entry, that becomes a read after delete.
  - The difftest skips (status -2) every region-propagation case with more than
    one other region, the caller as its own other region, a duplicate list entry,
    or a non-vector `SM_AI_COLOR` attribute. For merges, it skips sources that
    carry `SM_AI_OTHER_REGION_ID`, because the merge-region handler casts and
    dereferences without checks.
- **`SmSAGObject::IsKindOf` chains to `SmAObject::IsKindOf`**, which skips
  `SmOwningTopology` and `SmTopology`. `SmBrep::IsKindOf(SmTopology_TYPE)` is
  therefore FALSE upstream. The port keeps this.
- **`SmSAGObject::CacheMakeOrValidate` stores a newly built cache in `this`, not
  in the target object.** The two are the same object when it is called from
  `GetOrCreateObjectCache`.
- No `UNPORTED(geom.a_object)` stubs existed to wire. The attribute-list methods
  remain co-ported in `geom::attribute::AttributeStore`.
