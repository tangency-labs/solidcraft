//! A plugin with the `storage` capability keeps its data under its own `plugins/<plugin-id>/`
//! folder: `meta.json` (`plugin`, `name`, `plugin_version` and the data's own `format` number),
//! `data.json` (its main document), `blobs/<name>` (its files) and `cache/<name>` (files it can
//! make again). An older design has none, so nothing in it is rewritten. The last release opens a
//! design that has them with a warning about the newer format number, and keeps and saves back
//! every one of these files byte for byte, as it already does for any plugin data.
