# Native GPU production envelope

This is an owner-directed integration-branch implementation, not a release or a performance certification. See the batch ledger for actual execution evidence.

The native GPU path executor admits output up to 8192 pixels per side and 33,554,432 pixels in all, at most 8,000,000 triangles and 65,536 captured bodies, at most 2 GiB of retained source geometry, and a finite 128 GiB logical work/result pool. The Render UI offers samples through 16,384 spp; the existing API sample ceiling remains 65,536. Preparation, texture, HDR, accumulation, denoising and retained-frame estimates must fit together. No allocation is made merely by registering the larger pool. Raster and other registered executors retain their existing 512 MiB software pool unless their implementation explicitly supplies a larger finite envelope. CPU requests retain their selected Compact/Studio/Workstation profile.

The device is queried before requesting storage: buffer binding and maximum buffer size use actual adapter support, bounded by the finite job envelope. The storage binding limit never exceeds the requested maximum buffer size. This replaces the earlier artificial 1 GiB request cap; it does not allocate those buffers or guarantee physical memory is available. Geometry, BVH nodes, textures, environments, accumulation and denoise guides are individually checked before upload. Failure does not reduce geometry, shrink the image, switch the selected device or discard maps. This is not out-of-core rendering.

Saved GPU dimensions, native request admission, PNG encoding, UI controls and completed-image display share the same final-output envelope. Optional [X2 supersampling](render-quality-controls.md) doubles both internal dimensions, up to 16384 per side and 134,217,728 internal pixels; all pixel-dependent work and storage use that larger grid. PNG, EXR and diagnostic mask exports retain the selected output dimensions. The display may use the existing explicitly labelled reduced texture cache; export retains original completed bytes. Individual device support may admit less than the maximum saved intent.

At most 128 dielectric bodies can be selected. Certified nested media use a bounded sixteen-layer stack. Closed-medium topology, orientation, homogeneous face optics and conservative rejection of ambiguous intersections remain enforced. Exact captured ordinal and name are still required.

## Large films and bounded filter windows

Persistent accumulation and adaptive planes use row-aligned pages. Each page targets at most 1 GiB per buffer and is further restricted by the adapter's binding, allocation and readback limits, including optical scratch. Global pixel indices, camera coordinates and random sample identities do not change at page boundaries. A pass completes only after all of its pages and optical strata finish. The total film remains resident; paging does not stream it to disk or reduce total film storage.

Denoising and guide diagnostics use a reusable full-width row window. Three filter passes need a fourteen-row dependency halo on each side of the core: the radius is twice the sum of steps 1, 2 and 4. Guide generation uses global coordinates and the full-image footprint. Only the core enters the final image and diagnostic counts; halo rows are not counted again. Page cores contain complete X2 cells, and diagnostic classes retain categorical reconstruction. Raw and preview readback also process bounded pages. Admission includes persistent pages, per-page optical scratch and the peak reusable guide, filter, gather and readback storage.

The software envelope permits the requested 8K output, X2, 16,384 spp, three-pass denoising, raw EXR and diagnostic controls together. Actual admission still checks captured geometry/resources, supported material combinations, the adapter and native RAM.

CPU and registered GPU image hosts share a process-wide reservation ledger with
a 256 GiB software ceiling, in addition to their own software pools. Physical
capacity comes from a separate available-RAM observation, including Linux cgroup
limits when reported. The observation funds one allocation epoch. It stays fixed
while capture, prepared or queued jobs, running work, or reusable checkpoint
credit can still allocate. Once no unsettled allocation promises or active work
remain, the next admission refreshes the observation. Immutable retained images,
previews and metadata do not block that refresh; their software leases stay
charged while their old physical debits are excluded from the new epoch.

The `host_memory` receipt separates the full request from its already-held
credit and the additional bytes to reserve. Its remaining physical allowance is
`capacity_bytes - physical_debit_bytes`, saturating at zero, and applies to
`additional_bytes`. See [the receipt fields](render-quality-controls.md) for the
software and physical accounting definitions. An unavailable RAM query uses an
explicit fallback of at most 512 MiB, bounded by any reported cgroup limit. All
held leases count against that fallback; retaining results cannot renew it. An
observed zero capacity remains zero.

This observation cannot guarantee memory against unrelated processes allocating
later. Refusal preserves the selected values and reports needed and available
bytes; no image allocation is required for preflight.

## Reusable headless studio

`solidcraft-cli mcp --in model.sc --render-gpu --allow-path OUTPUT_DIRECTORY` explicitly installs the native path-tracing host once. Subsequent `render.scene`, `render.preflight`, `render.start`, `render.status` and `render.export` commands reuse the same evaluated native design and device. Without `--render-gpu`, MCP does not discover a path-tracing GPU. The flag is rejected with `--connect`, because a connected application owns its device. `SOLIDCRAFT_GPU` is respected; hardware-only workflows use `hardware` and never substitute software.

`tools/render-library/selene_studio.py` is a development-only orchestration helper for the pinned functional master. Start it with `python -i`, an explicitly selected freshly built CLI, and a new output directory. Review `studio.scene`, submit an explicit request with `studio.render(label, params)`, and call `studio.close()`. The helper records the unmodified master hash, executable hash, exact request, preflight, job identity, exported PNG hash and completion state. Numerical completion is not visual approval.

This entry point supports coordinated camera/material/light iterations without reopening a large CAD assembly or relinking the UI. The unmodified source model remains the authority. Render copies, receipts and image outputs are retained separately; only production resources and reproducible authoring source belong in Git.
