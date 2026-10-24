//! Named render setups can retain a typed active raster, CPU raytrace or GPU path configuration.
//! Only bounded numerical options and the procedural sky are portable; captured bindings and
//! external HDR/maps remain runtime-only. Exposure has one saved authority in RenderSetup.
//! Older designs require no transformation: an absent/null job keeps the legacy protocol.
//! The last release refuses the newer format number; pending builds without this descriptor
//! refuse the design instead of discarding its job.
