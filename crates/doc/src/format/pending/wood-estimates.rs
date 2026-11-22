//! Recognises per-design finishing supplies in metadata.io.solidcraft.wood.estimate:
//! an object containing a finishing array of name, quantity and unit_price records.
//! Designs without this metadata use their shared stock library's supplies, or none.
//! The last release warns about the newer format, opens what it understands and
//! preserves the namespaced metadata it cannot interpret when saving again.
