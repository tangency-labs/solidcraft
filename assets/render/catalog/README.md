# Runtime render catalog exports

These JSON files are necessary runtime exports of the editable authoring catalogs in
`docs/render-materials/`. Do not edit both copies. `python -B tools/render-library/generate.py`
copies the original bytes and updates the Rust lookup; `--check` verifies exact equality.
The application includes these asset exports, never files from the documentation tree.

The 192 texture maps and 24 HDR files are separate from this metadata. They are included
only with the engine's `bundled-render-library` feature. Desktop builds enable it by
default. Standalone CLI and web builds do not; use their same-named feature to opt in.
Discovery and uniform appearances remain available without the image pack. Requests
requiring missing bytes fail explicitly before document mutation; no network is used.
GPU/UI test targets opt in deliberately. Exercise the engine's no-pack negative tests
separately from its feature-enabled resource tests.
