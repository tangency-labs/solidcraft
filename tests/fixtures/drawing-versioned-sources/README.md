# Versioned drawing and source design acceptance

Review package for the first contribution discussed in [#3890](https://github.com/tangency-labs/solidcraft/issues/3890#issuecomment-6363836282). Published as draft PR #5430 and issue #5428; this update follows the implemented K2 contract.

- [Acceptance contract](docs/acceptance.md): the four agreed cases and the evidence required from their future implementation.
- [Fixture guide](fixtures/README.md): two source designs, two historical states, path examples and the confirmed K2 fields and remaining integration bindings.
- [Scenario data](fixtures/scenarios.json): inputs and expected outcomes for a future harness; not a SolidCraft command script.
- `fixtures/C0/` and `fixtures/C1/`: native source-model JSON and illustrative drawing-record templates.
- [Binding requirements](fixtures/bindings.json): symbolic fixture identities and the values the Drawing implementation must supply.

The source designs use the existing legacy `solidcraft/1` input shape. The drawing files end in `.template.json`: their K2 record fields are bound, but they are **not validated application/container files**. This package uses the owner-defined format-1 record and ID representation; it defines no cache encoding, release policy or new application API.

The two snapshot directories are logical repository states, not actual Git commits. A future harness will create two commits in a disposable repository and use the application/storage APIs once available. Do not open these templates as if they were current production drawing files.

This contribution follows the acceptance-document/fixture approach used in [#4962](https://github.com/tangency-labs/solidcraft/pull/4962), without copying its geometry fixtures or extending its implementation scope. The existing draft PR #5430 targets `drawings` and remains documentation/examples only.

K2 update: examples now carry 13-character nonzero drawing IDs, sheet-level `dpi: 300`, `excluded: {count: false, print: false}`, fixed-mm units, A4 landscape, no border, and `documents: {source, component: 0}`. K3 container/historical integration remains pending; no runtime behavior is claimed. See the acceptance contract and binding notes.
