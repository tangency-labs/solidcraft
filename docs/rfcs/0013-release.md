# RFC 0013: Release and compliance

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P6 of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0012 Test](0012-test.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0013--release-and-compliance) |

## Summary

A product is released when someone else can build it. That means a frozen revision, complete
documentation, compliance evidence, packaging, volume cost, a manifest with checksums, and a review.
This is a checklist and a manifest, not a gate.

## Scope

- Freeze a revision and record what changed and why.
- Production documentation: drawings, instructions, test procedure, labels.
- Compliance evidence: RoHS/REACH, FCC/CE/UKCA, battery, packaging, safety; screening, not
  certification.
- Packaging and labels.
- Volume cost, sourcing, alternates.
- Release manifest: outputs, checksums, notes, build instructions, release notes.
- Git-friendly project, design diffs, comments, review before release.
- An optional release review.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation. `release.verify` must be usable in
CI with a non-zero exit on an open must requirement.

## Readiness (optional)

A third party could build the product from the release. Release stays available without a result or
a note; missing evidence is listed, never enforced as a stop.
