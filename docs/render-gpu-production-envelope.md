# Native GPU production envelope

This is an owner-directed integration-branch implementation, not a release or a performance certification. See the batch ledger for actual execution evidence.

The native GPU path executor admits output up to 8192 pixels per side and 16,777,216 pixels in all, at most 8,000,000 triangles and 65,536 captured bodies, at most 2 GiB of retained source geometry, and an 8 GiB logical work/result pool. All maxima interact with the full preparation, texture, HDR, accumulation, denoising and retained-frame estimates. No allocation is made merely by registering the larger pool. Raster and other registered executors retain their existing 512 MiB default unless their implementation explicitly supplies a larger finite envelope. CPU requests retain their selected Compact/Studio/Workstation profile.

The Vulkan device is queried before requesting storage: buffer binding and maximum buffer size are limited to the lesser of actual support and 1 GiB. Geometry, BVH nodes, textures, environments, accumulation and denoise guides are individually checked before upload. Failure does not reduce geometry, shrink the image, switch the selected device or discard maps. This is not out-of-core rendering.

Saved GPU dimensions, native request admission, PNG encoding, UI controls and completed-image display share the same envelope. The display may use the existing explicitly labelled reduced texture cache; export retains original completed bytes. Individual device support may admit less than the maximum saved intent.

At most 32 disjoint dielectric bodies can be selected. Closed-medium topology, orientation, homogeneous face optics and conservative medium-overlap rejection remain unchanged. Increasing object cardinality does not implement nested media or change the shader's path stack. Exact captured ordinal and name are still required.

## Reusable headless studio

`solidcraft-cli mcp --in model.solidcraft --render-gpu --allow-path OUTPUT_DIRECTORY` explicitly installs the native path-tracing host once. Subsequent `render.scene`, `render.preflight`, `render.start`, `render.status` and `render.export` commands reuse the same evaluated native design and device. Without `--render-gpu`, MCP does not discover a path-tracing GPU. The flag is rejected with `--connect`, because a connected application owns its device. `SOLIDCRAFT_GPU` is respected; hardware-only workflows use `hardware` and never substitute software.

`tools/render-library/selene_studio.py` is a development-only orchestration helper for the pinned functional master. Start it with `python -i`, an explicitly selected freshly built CLI, and a new output directory. Review `studio.scene`, submit an explicit request with `studio.render(label, params)`, and call `studio.close()`. The helper records the unmodified master hash, executable hash, exact request, preflight, job identity, exported PNG hash and completion state. Numerical completion is not visual approval.

This entry point supports coordinated camera/material/light iterations without reopening a large CAD assembly or relinking the UI. The unmodified source model remains the authority. Render copies, receipts and image outputs are retained separately; only production resources and reproducible authoring source belong in Git.
