//! A design can hold CAM milling data (`cam`: tools, setups, and 2D contour/pocket operations).
//! A setup is an ISO 841 work frame in the model (`work_origin`, `tool_axis`, optional `x_axis`)
//! plus an RS-274 work offset (`G54`…`G59`) and an optional stock box. Sketch and face geometry
//! is mapped into that frame when posting; explicit polylines are already work XY. Depths, feeds
//! and stepdowns are expressions evaluated when posting. Toolpaths are never saved. Older designs
//! need no transformation: an absent `cam` field means no setups. The last release refuses a
//! design saved with this (the newer format number).
