//! NURBS curve tools: knot insertion, refinement and removal, splitting, segment
//! extraction, Bezier decomposition and degree elevation.
//!
//! Upstream: `source/SMLib/src/NL_CrvTool.cpp`, split into the plan units
//! `nlib.crv_tool_p1`, `nlib.crv_tool_p2` (`plan/modules.toml`).

pub mod p1;
