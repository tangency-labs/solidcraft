//! Save a weld's pass layout: `passes`, passes per layer, root first, shown when `passes_shown`
//! is on (fillet, butt, edge, plug and surfacing welds). Older designs have none and show one
//! pass; a weld without a layout saves no `passes`. The last release warns about the newer
//! format and preserves the weld data it cannot read when saving.
